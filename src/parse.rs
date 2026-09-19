use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub number: i64,
    pub field_type: String,
    pub repeated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumValue {
    pub name: String,
    pub number: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Enum {
    pub name: String,
    pub values: Vec<EnumValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Method {
    pub name: String,
    pub input_type: String,
    pub output_type: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    pub name: String,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProtoFile {
    pub messages: Vec<Message>,
    pub enums: Vec<Enum>,
    pub services: Vec<Service>,
}

/// Strips `//` and `/* */` comments, replacing their content with
/// spaces (newlines preserved) rather than deleting it outright, so
/// every later byte offset used for depth-tracking still lines up with
/// the original text. String literals are left untouched by comment
/// stripping (a `"//"` inside a default-value string must not be
/// mistaken for a comment start) but are otherwise not specially
/// parsed — this tool doesn't need their contents, only that they
/// don't confuse the comment scanner.
fn strip_comments(text: &str) -> String {
    #[derive(PartialEq, Clone, Copy)]
    enum State {
        Normal,
        Str,
        Line,
        Block,
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut state = State::Normal;

    while let Some(c) = chars.next() {
        match state {
            State::Normal => match c {
                '"' => {
                    state = State::Str;
                    out.push(c);
                }
                '/' if chars.peek() == Some(&'/') => {
                    chars.next();
                    out.push(' ');
                    out.push(' ');
                    state = State::Line;
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    out.push(' ');
                    out.push(' ');
                    state = State::Block;
                }
                _ => out.push(c),
            },
            State::Str => {
                out.push(c);
                if c == '"' {
                    state = State::Normal;
                }
            }
            State::Line => {
                if c == '\n' {
                    out.push('\n');
                    state = State::Normal;
                } else {
                    out.push(' ');
                }
            }
            State::Block => {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    out.push(' ');
                    out.push(' ');
                    state = State::Normal;
                } else if c == '\n' {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
        }
    }
    out
}

fn depth_at(text: &str, pos: usize) -> i32 {
    let mut depth = 0;
    for c in text[..pos].chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// Finds the byte offset of the `}` that closes the `{` immediately
/// preceding `body_start` (i.e. `body_start` is the offset right after
/// that opening brace).
fn find_block_end(text: &str, body_start: usize) -> usize {
    let mut depth = 1;
    for (i, c) in text[body_start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return body_start + i;
                }
            }
            _ => {}
        }
    }
    text.len()
}

static BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(message|enum|service)\s+(\w+)\s*\{").unwrap());

/// Extracts every **top-level** `message`/`enum`/`service` block —
/// nested messages, nested enums, and `oneof` groups are deliberately
/// out of scope for this v1 (see README) and are identified and
/// skipped here by depth: only a match at brace depth 0 counts as
/// top-level.
fn top_level_blocks(text: &str) -> Vec<(&'static str, String, String)> {
    let mut results = Vec::new();
    for m in BLOCK_RE.captures_iter(text) {
        let whole = m.get(0).unwrap();
        if depth_at(text, whole.start()) != 0 {
            continue;
        }
        let kind = match &m[1] {
            "message" => "message",
            "enum" => "enum",
            "service" => "service",
            _ => unreachable!(),
        };
        let name = m[2].to_string();
        let body_start = whole.end();
        let body_end = find_block_end(text, body_start);
        results.push((kind, name, text[body_start..body_end].to_string()));
    }
    results
}

// No `^`/line-start anchor: real .proto files are one-field-per-line
// almost universally, but nothing requires it, and a compact or
// minified file with multiple fields on one line would otherwise
// silently lose every field after the first on that line (caught by
// this crate's own diff tests, several of which build single-line
// inline `.proto` text for brevity). The brace-depth filter applied
// where this regex is used, plus requiring the full `type name =
// number;` shape, is specific enough to not need a position anchor.
static FIELD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(repeated\s+|optional\s+)?([\w.]+)\s+(\w+)\s*=\s*(-?\d+)\s*(\[[^\]]*\])?\s*;")
        .unwrap()
});

fn parse_fields(body: &str) -> Vec<Field> {
    FIELD_RE
        .captures_iter(body)
        .filter(|m| depth_at(body, m.get(0).unwrap().start()) == 0)
        .map(|m| Field {
            name: m[3].to_string(),
            number: m[4].parse().unwrap_or(0),
            field_type: m[2].to_string(),
            repeated: m.get(1).is_some_and(|g| g.as_str().trim() == "repeated"),
        })
        .collect()
}

static ENUM_VALUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\w+)\s*=\s*(-?\d+)\s*(\[[^\]]*\])?\s*;").unwrap());

fn parse_enum_values(body: &str) -> Vec<EnumValue> {
    ENUM_VALUE_RE
        .captures_iter(body)
        .filter(|m| depth_at(body, m.get(0).unwrap().start()) == 0)
        .map(|m| EnumValue {
            name: m[1].to_string(),
            number: m[2].parse().unwrap_or(0),
        })
        .collect()
}

static RPC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)rpc\s+(\w+)\s*\(\s*(stream\s+)?([\w.]+)\s*\)\s*returns\s*\(\s*(stream\s+)?([\w.]+)\s*\)",
    )
    .unwrap()
});

fn parse_methods(body: &str) -> Vec<Method> {
    RPC_RE
        .captures_iter(body)
        .map(|m| Method {
            name: m[1].to_string(),
            client_streaming: m.get(2).is_some(),
            input_type: m[3].to_string(),
            server_streaming: m.get(4).is_some(),
            output_type: m[5].to_string(),
        })
        .collect()
}

pub fn parse_proto(text: &str) -> ProtoFile {
    let clean = strip_comments(text);
    let mut file = ProtoFile::default();

    for (kind, name, body) in top_level_blocks(&clean) {
        match kind {
            "message" => file.messages.push(Message {
                name,
                fields: parse_fields(&body),
            }),
            "enum" => file.enums.push(Enum {
                name,
                values: parse_enum_values(&body),
            }),
            "service" => file.services.push(Service {
                name,
                methods: parse_methods(&body),
            }),
            _ => unreachable!(),
        }
    }

    file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_realistic_message() {
        let text = r#"
message HelloRequest {
  string name = 1;
  repeated int32 favorite_numbers = 2;
}
"#;
        let file = parse_proto(text);
        assert_eq!(file.messages.len(), 1);
        let msg = &file.messages[0];
        assert_eq!(msg.name, "HelloRequest");
        assert_eq!(msg.fields.len(), 2);
        assert_eq!(msg.fields[0].name, "name");
        assert_eq!(msg.fields[0].number, 1);
        assert!(!msg.fields[0].repeated);
        assert_eq!(msg.fields[1].name, "favorite_numbers");
        assert!(msg.fields[1].repeated);
    }

    #[test]
    fn parses_multiple_fields_packed_onto_a_single_line() {
        // A real gap this caught live: the original FIELD_RE required
        // each match to start at the beginning of a line, so only the
        // first field on a compact/single-line message was ever seen.
        let file = parse_proto("message A { string x = 1; string y = 2; }");
        assert_eq!(file.messages[0].fields.len(), 2);
        assert_eq!(file.messages[0].fields[1].name, "y");
        assert_eq!(file.messages[0].fields[1].number, 2);
    }

    #[test]
    fn parses_an_enum() {
        let text = r#"
enum Status {
  UNKNOWN = 0;
  ACTIVE = 1;
  BANNED = 2;
}
"#;
        let file = parse_proto(text);
        assert_eq!(file.enums.len(), 1);
        assert_eq!(file.enums[0].values.len(), 3);
        assert_eq!(file.enums[0].values[2].name, "BANNED");
        assert_eq!(file.enums[0].values[2].number, 2);
    }

    #[test]
    fn parses_a_service_with_streaming_flags() {
        let text = r#"
service Greeter {
  rpc SayHello (HelloRequest) returns (HelloReply);
  rpc Chat (stream ChatMessage) returns (stream ChatMessage);
}
"#;
        let file = parse_proto(text);
        assert_eq!(file.services.len(), 1);
        let svc = &file.services[0];
        assert_eq!(svc.methods.len(), 2);
        assert!(!svc.methods[0].client_streaming);
        assert!(!svc.methods[0].server_streaming);
        assert!(svc.methods[1].client_streaming);
        assert!(svc.methods[1].server_streaming);
    }

    #[test]
    fn nested_message_fields_are_not_pulled_into_the_outer_message() {
        let text = r#"
message Outer {
  string id = 1;
  message Inner {
    string value = 1;
  }
}
"#;
        let file = parse_proto(text);
        assert_eq!(
            file.messages.len(),
            1,
            "nested Inner is deliberately not parsed as top-level"
        );
        assert_eq!(file.messages[0].fields.len(), 1);
        assert_eq!(file.messages[0].fields[0].name, "id");
    }

    #[test]
    fn line_comments_do_not_confuse_field_parsing() {
        let text = r#"
message M {
  string name = 1; // this used to be called full_name
}
"#;
        let file = parse_proto(text);
        assert_eq!(file.messages[0].fields.len(), 1);
    }

    #[test]
    fn block_comments_do_not_confuse_field_parsing() {
        let text = r#"
message M {
  /* deprecated, kept for wire compat */
  string name = 1;
}
"#;
        let file = parse_proto(text);
        assert_eq!(file.messages[0].fields.len(), 1);
    }

    #[test]
    fn a_string_literal_containing_slash_slash_does_not_start_a_fake_comment() {
        let text = r#"
message M {
  string url = 1 [default = "http://example.com"];
}
"#;
        let file = parse_proto(text);
        assert_eq!(file.messages[0].fields.len(), 1);
        assert_eq!(file.messages[0].fields[0].name, "url");
    }

    #[test]
    fn field_options_in_brackets_do_not_break_parsing() {
        let text = r#"
message M {
  string id = 1 [json_name = "identifier", deprecated = true];
}
"#;
        let file = parse_proto(text);
        assert_eq!(file.messages[0].fields.len(), 1);
    }
}
