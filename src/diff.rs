use crate::parse::{Enum, Message, ProtoFile, Service};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Changes the wire format or RPC routing in a way that breaks an
    /// existing client compiled against the old `.proto`.
    Breaking,
    /// Wire-compatible, but risky for anything that keys off names
    /// instead of numbers — proto3 canonical JSON, reflection-based
    /// tooling, generated code callers who used the field/method name.
    Warning,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Severity::Breaking => "BREAKING",
            Severity::Warning => "WARNING",
        })
    }
}

pub struct Finding {
    pub severity: Severity,
    pub summary: String,
}

fn diff_message(old: &Message, new: &Message, findings: &mut Vec<Finding>) {
    for old_field in &old.fields {
        match new.fields.iter().find(|f| f.number == old_field.number) {
            None => findings.push(Finding {
                severity: Severity::Breaking,
                summary: format!(
                    "{}: field {} '{}' removed (or its number reused)",
                    old.name, old_field.number, old_field.name
                ),
            }),
            Some(new_field) => {
                if new_field.field_type != old_field.field_type {
                    findings.push(Finding {
                        severity: Severity::Breaking,
                        summary: format!(
                            "{}: field {} type changed: {} -> {}",
                            old.name, old_field.number, old_field.field_type, new_field.field_type
                        ),
                    });
                }
                if new_field.repeated != old_field.repeated {
                    findings.push(Finding {
                        severity: Severity::Breaking,
                        summary: format!(
                            "{}: field {} cardinality changed: {} -> {}",
                            old.name,
                            old_field.number,
                            if old_field.repeated {
                                "repeated"
                            } else {
                                "singular"
                            },
                            if new_field.repeated {
                                "repeated"
                            } else {
                                "singular"
                            },
                        ),
                    });
                }
                if new_field.name != old_field.name {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        summary: format!(
                            "{}: field {} renamed: '{}' -> '{}' (wire-compatible, but changes proto3 JSON field name)",
                            old.name, old_field.number, old_field.name, new_field.name
                        ),
                    });
                }
            }
        }
    }
}

fn diff_enum(old: &Enum, new: &Enum, findings: &mut Vec<Finding>) {
    for old_value in &old.values {
        match new.values.iter().find(|v| v.number == old_value.number) {
            None => findings.push(Finding {
                severity: Severity::Breaking,
                summary: format!(
                    "{}: enum value {} '{}' removed (or its number reused)",
                    old.name, old_value.number, old_value.name
                ),
            }),
            Some(new_value) => {
                if new_value.name != old_value.name {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        summary: format!(
                            "{}: enum value {} renamed: '{}' -> '{}'",
                            old.name, old_value.number, old_value.name, new_value.name
                        ),
                    });
                }
            }
        }
    }
}

fn diff_service(old: &Service, new: &Service, findings: &mut Vec<Finding>) {
    for old_method in &old.methods {
        match new.methods.iter().find(|m| m.name == old_method.name) {
            None => findings.push(Finding {
                severity: Severity::Breaking,
                summary: format!("{}: method '{}' removed", old.name, old_method.name),
            }),
            Some(new_method) => {
                if new_method.input_type != old_method.input_type {
                    findings.push(Finding {
                        severity: Severity::Breaking,
                        summary: format!(
                            "{}.{}: input type changed: {} -> {}",
                            old.name, old_method.name, old_method.input_type, new_method.input_type
                        ),
                    });
                }
                if new_method.output_type != old_method.output_type {
                    findings.push(Finding {
                        severity: Severity::Breaking,
                        summary: format!(
                            "{}.{}: output type changed: {} -> {}",
                            old.name,
                            old_method.name,
                            old_method.output_type,
                            new_method.output_type
                        ),
                    });
                }
                if new_method.client_streaming != old_method.client_streaming
                    || new_method.server_streaming != old_method.server_streaming
                {
                    findings.push(Finding {
                        severity: Severity::Breaking,
                        summary: format!(
                            "{}.{}: streaming shape changed (client_streaming: {} -> {}, server_streaming: {} -> {})",
                            old.name,
                            old_method.name,
                            old_method.client_streaming,
                            new_method.client_streaming,
                            old_method.server_streaming,
                            new_method.server_streaming,
                        ),
                    });
                }
            }
        }
    }
}

pub fn diff(old: &ProtoFile, new: &ProtoFile) -> Vec<Finding> {
    let mut findings = Vec::new();

    for old_msg in &old.messages {
        match new.messages.iter().find(|m| m.name == old_msg.name) {
            None => findings.push(Finding {
                severity: Severity::Breaking,
                summary: format!("message '{}' removed", old_msg.name),
            }),
            Some(new_msg) => diff_message(old_msg, new_msg, &mut findings),
        }
    }

    for old_enum in &old.enums {
        match new.enums.iter().find(|e| e.name == old_enum.name) {
            None => findings.push(Finding {
                severity: Severity::Breaking,
                summary: format!("enum '{}' removed", old_enum.name),
            }),
            Some(new_enum) => diff_enum(old_enum, new_enum, &mut findings),
        }
    }

    for old_svc in &old.services {
        match new.services.iter().find(|s| s.name == old_svc.name) {
            None => findings.push(Finding {
                severity: Severity::Breaking,
                summary: format!("service '{}' removed", old_svc.name),
            }),
            Some(new_svc) => diff_service(old_svc, new_svc, &mut findings),
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_proto;

    #[test]
    fn removed_message_is_breaking() {
        let old = parse_proto("message A { string x = 1; }\nmessage B { string y = 1; }");
        let new = parse_proto("message A { string x = 1; }");
        let findings = diff(&old, &new);
        assert!(findings
            .iter()
            .any(|f| f.severity == Severity::Breaking && f.summary.contains("'B' removed")));
    }

    #[test]
    fn removed_field_is_breaking() {
        let old = parse_proto("message A { string x = 1; string y = 2; }");
        let new = parse_proto("message A { string x = 1; }");
        let findings = diff(&old, &new);
        assert!(findings
            .iter()
            .any(|f| f.severity == Severity::Breaking && f.summary.contains("field 2")));
    }

    #[test]
    fn field_type_change_is_breaking() {
        let old = parse_proto("message A { string x = 1; }");
        let new = parse_proto("message A { int32 x = 1; }");
        let findings = diff(&old, &new);
        assert!(findings
            .iter()
            .any(|f| f.severity == Severity::Breaking && f.summary.contains("type changed")));
    }

    #[test]
    fn cardinality_change_is_breaking() {
        let old = parse_proto("message A { string x = 1; }");
        let new = parse_proto("message A { repeated string x = 1; }");
        let findings = diff(&old, &new);
        assert!(findings
            .iter()
            .any(|f| f.severity == Severity::Breaking && f.summary.contains("cardinality")));
    }

    #[test]
    fn field_rename_with_same_number_and_type_is_only_a_warning() {
        let old = parse_proto("message A { string x = 1; }");
        let new = parse_proto("message A { string renamed_x = 1; }");
        let findings = diff(&old, &new);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    #[test]
    fn a_newly_added_field_is_not_flagged_at_all() {
        let old = parse_proto("message A { string x = 1; }");
        let new = parse_proto("message A { string x = 1; string y = 2; }");
        let findings = diff(&old, &new);
        assert!(findings.is_empty());
    }

    #[test]
    fn removed_enum_value_is_breaking() {
        let old = parse_proto("enum E { A = 0; B = 1; }");
        let new = parse_proto("enum E { A = 0; }");
        let findings = diff(&old, &new);
        assert!(findings
            .iter()
            .any(|f| f.severity == Severity::Breaking && f.summary.contains("enum value 1")));
    }

    #[test]
    fn removed_rpc_method_is_breaking() {
        let old =
            parse_proto("service S { rpc A (Req) returns (Res); rpc B (Req) returns (Res); }");
        let new = parse_proto("service S { rpc A (Req) returns (Res); }");
        let findings = diff(&old, &new);
        assert!(findings
            .iter()
            .any(|f| f.severity == Severity::Breaking && f.summary.contains("'B' removed")));
    }

    #[test]
    fn rpc_input_type_change_is_breaking() {
        let old = parse_proto("service S { rpc A (Req) returns (Res); }");
        let new = parse_proto("service S { rpc A (NewReq) returns (Res); }");
        let findings = diff(&old, &new);
        assert!(findings
            .iter()
            .any(|f| f.severity == Severity::Breaking && f.summary.contains("input type changed")));
    }

    #[test]
    fn rpc_becoming_streaming_is_breaking() {
        let old = parse_proto("service S { rpc A (Req) returns (Res); }");
        let new = parse_proto("service S { rpc A (Req) returns (stream Res); }");
        let findings = diff(&old, &new);
        assert!(findings
            .iter()
            .any(|f| f.severity == Severity::Breaking && f.summary.contains("streaming shape")));
    }

    #[test]
    fn identical_files_produce_no_findings() {
        let text =
            "message A { string x = 1; }\nenum E { A = 0; }\nservice S { rpc M (A) returns (A); }";
        let old = parse_proto(text);
        let new = parse_proto(text);
        assert!(diff(&old, &new).is_empty());
    }
}
