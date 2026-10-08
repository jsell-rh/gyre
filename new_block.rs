    /// Wire type of a payload field, per message-bus.md §Payload Schemas.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FieldType {
        /// `Id` or `String` — any JSON string.
        Str,
        /// `u64` — a JSON number that is a non-negative integer.
        U64,
        /// `u32` — a JSON number that is a non-negative integer < 2^32.
        U32,
        /// `f64` — any JSON number (integer literals like `usage_pct: 80` are valid).
        F64,
        /// `Vec<String>` — a JSON array whose elements are all strings.
        StrArray,
        /// `decisions: [{what, why, confidence, alternatives_considered?}]` —
        /// a JSON array of objects each carrying string `what`/`why`/`confidence`
        /// and an optional string-array `alternatives_considered`.
        Decisions,
    }

    /// One payload field's schema: name, wire type, requiredness.
    struct FieldSpec {
        name: &'static str,
        ty: FieldType,
        required: bool,
    }

    const fn f(name: &'static str, ty: FieldType, required: bool) -> FieldSpec {
        FieldSpec { name, ty, required }
    }

    /// Payload schema for this kind — message-bus.md §Payload Schemas
    /// (lines 194–229), transcribed field-by-field including wire types.
    ///
    /// Returns `None` for kinds the spec table does not list (server-emitted
    /// only: `SpecApproved`, `ConstraintViolation`, `AtomicGroupFailed`,
    /// `MrReverted`, `MergeQueuePaused`, `MergeQueueResumed`) and for `Custom`,
    /// which accepts any JSON object — both keep explicit match arms below so
    /// adding a spec row for one is a visible edit, not a silent default.
    fn payload_schema(&self) -> Option<&'static [FieldSpec]> {
        use FieldType::*;
        match self {
            // Directed
            MessageKind::TaskAssignment => Some(&[
                f("task_id", Str, true),
                f("spec_ref", Str, false),
            ]),
            MessageKind::ReviewRequest => Some(&[f("mr_id", Str, true)]),
            MessageKind::StatusUpdate => Some(&[
                f("status", Str, true),
                f("summary", Str, true),
            ]),
            MessageKind::Escalation => Some(&[
                f("reason", Str, true),
                f("context", Str, false),
            ]),
            // Events
            MessageKind::AgentCreated => Some(&[f("agent_id", Str, true)]),
            MessageKind::AgentStatusChanged => Some(&[
                f("agent_id", Str, true),
                f("status", Str, true),
            ]),
            MessageKind::AgentContainerSpawned => Some(&[
                f("agent_id", Str, true),
                f("container_id", Str, true),
                f("image", Str, true),
                f("runtime", Str, true),
            ]),
            MessageKind::AgentCompleted => Some(&[
                f("agent_id", Str, true),
                f("task_id", Str, true),
                f("spec_ref", Str, false),
                f("decisions", Decisions, false),
                f("uncertainties", StrArray, false),
                f("conversation_sha", Str, false),
            ]),
            MessageKind::ReconciliationCompleted => Some(&[
                f("workspace_id", Str, true),
                f("persona_id", Str, true),
                f("persona_name", Str, false),
                f("specs_evaluated", U32, false),
                f("specs_changed", U32, false),
                f("preview_branch", Str, false),
            ]),
            MessageKind::TaskCreated => Some(&[f("task_id", Str, true)]),
            MessageKind::TaskTransitioned => Some(&[
                f("task_id", Str, true),
                f("status", Str, true),
            ]),
            MessageKind::MrCreated => Some(&[f("mr_id", Str, true)]),
            MessageKind::MrStatusChanged => Some(&[
                f("mr_id", Str, true),
                f("status", Str, true),
            ]),
            MessageKind::MrMerged => Some(&[
                f("mr_id", Str, true),
                f("merge_commit_sha", Str, false),
            ]),
            MessageKind::PushRejected => Some(&[
                f("repo_id", Str, true),
                f("branch", Str, true),
                f("agent_id", Str, true),
                f("reason", Str, true),
            ]),
            MessageKind::PushAccepted => Some(&[
                f("repo_id", Str, true),
                f("branch", Str, true),
                f("agent_id", Str, true),
                f("commit_count", U64, false),
                f("task_id", Str, false),
                f("ralph_step", Str, false),
            ]),
            MessageKind::SpecChanged => Some(&[
                f("repo_id", Str, true),
                f("spec_path", Str, true),
                f("change_kind", Str, true),
                f("task_id", Str, false),
                f("dependent_workspace_id", Str, false),
                f("source_workspace_slug", Str, false),
            ]),
            MessageKind::GateFailure => Some(&[
                f("mr_id", Str, true),
                f("gate_name", Str, true),
                f("gate_type", Str, false),
                f("status", Str, false),
                f("output", Str, false),
                f("spec_ref", Str, false),
                f("gate_agent_id", Str, false),
            ]),
            MessageKind::StaleSpecWarning => Some(&[
                f("mr_id", Str, true),
                f("repo_id", Str, true),
                f("spec_path", Str, true),
                f("spec_sha", Str, true),
                f("current_sha", Str, true),
            ]),
            MessageKind::SpeculativeConflict => Some(&[
                f("repo_id", Str, true),
                f("branch", Str, true),
                f("conflicting_files", StrArray, true),
            ]),
            MessageKind::SpeculativeMergeClean => Some(&[
                f("repo_id", Str, true),
                f("branch", Str, true),
            ]),
            MessageKind::HotFilesChanged => Some(&[f("repo_id", Str, true)]),
            MessageKind::BudgetWarning => Some(&[
                f("agent_id", Str, true),
                f("workspace_id", Str, true),
                f("usage_pct", F64, true),
            ]),
            MessageKind::BudgetExhausted => Some(&[
                f("agent_id", Str, true),
                f("workspace_id", Str, true),
                f("grace_secs", U64, true),
            ]),
            MessageKind::AgentError => Some(&[
                f("agent_id", Str, true),
                f("error", Str, true),
                f("context", Str, false),
            ]),
            // Telemetry
            MessageKind::ToolCallStart => Some(&[
                f("agent_id", Str, true),
                f("tool_name", Str, true),
            ]),
            MessageKind::ToolCallEnd => Some(&[
                f("agent_id", Str, true),
                f("tool_name", Str, true),
                f("duration_ms", U64, true),
            ]),
            MessageKind::TextMessageContent => Some(&[
                f("agent_id", Str, true),
                f("content", Str, true),
                f("role", Str, false),
            ]),
            MessageKind::RunStarted => Some(&[
                f("agent_id", Str, true),
                f("task_id", Str, false),
            ]),
            MessageKind::RunFinished => Some(&[
                f("agent_id", Str, true),
                f("task_id", Str, false),
            ]),
            MessageKind::StateChanged => Some(&[
                f("agent_id", Str, true),
                f("old_state", Str, false),
                f("new_state", Str, true),
            ]),
            // Spec table lists no payload fields.
            MessageKind::QueueUpdated | MessageKind::DataSeeded => Some(&[]),
            // Outside the spec table (server-emitted only) — no specced schema.
            MessageKind::SpecApproved
            | MessageKind::ConstraintViolation
            | MessageKind::AtomicGroupFailed
            | MessageKind::MrReverted
            | MessageKind::MergeQueuePaused
            | MessageKind::MergeQueueResumed => None,
            // Any JSON object; the payload shape is the sender's contract.
            MessageKind::Custom(_) => None,
        }
    }

    /// Check one field value against its declared wire type; the `Err` carries
    /// the human-readable type name for the rejection reason.
    fn check_field_type(ty: FieldType, value: &Value) -> Result<(), &'static str> {
        let ok = match ty {
            FieldType::Str => value.is_string(),
            FieldType::U64 => value.as_u64().is_some(),
            FieldType::U32 => matches!(value.as_u64(), Some(n) if n <= u32::MAX as u64),
            FieldType::F64 => value.as_f64().is_some(),
            FieldType::StrArray => value
                .as_array()
                .map_or(false, |arr| arr.iter().all(|v| v.is_string())),
            FieldType::Decisions => value.as_array().map_or(false, |arr| {
                arr.iter().all(|v| {
                    v.as_object().map_or(false, |o| {
                        o.get("what").map_or(false, |x| x.is_string())
                            && o.get("why").map_or(false, |x| x.is_string())
                            && o.get("confidence").map_or(false, |x| x.is_string())
                            && match o.get("alternatives_considered") {
                                None | Some(Value::Null) => true,
                                Some(a) => a
                                    .as_array()
                                    .map_or(false, |arr| arr.iter().all(|x| x.is_string())),
                            }
                    })
                })
            }),
        };
        if ok {
            Ok(())
        } else {
            Err(match ty {
                FieldType::Str => "a string",
                FieldType::U64 => "an unsigned integer",
                FieldType::U32 => "an unsigned 32-bit integer",
                FieldType::F64 => "a number",
                FieldType::StrArray => "an array of strings",
                FieldType::Decisions => "an array of decision objects",
            })
        }
    }

    /// Validate a payload received from a caller against this kind's schema
    /// (message-bus.md §Payload Schemas): the payload must be a JSON object,
    /// every required field must be present and non-null, and every
    /// schema-known field that is present (required or optional) must match
    /// its declared wire type.
    ///
    /// Unknown extension fields pass through untouched, and an optional field
    /// may be absent (or explicitly null — null is treated as absent, the same
    /// normalization the payload itself gets). An absent payload is valid only
    /// for kinds with no required fields. The `Err` string is a human-readable
    /// reason meant to be surfaced verbatim to the caller (REST 400 body /
    /// MCP `tool_error`).
    pub fn validate_payload(&self, payload: Option<&Value>) -> Result<(), String> {
        let schema = match self.payload_schema() {
            None => {
                // No specced schema (`Custom`, non-spec server-emitted kinds):
                // any JSON object is accepted; so is an absent payload.
                return match payload.filter(|v| !v.is_null()) {
                    None | Some(Value::Object(_)) => Ok(()),
                    Some(_) => Err(format!(
                        "payload for kind '{}' must be a JSON object",
                        self.as_str()
                    )),
                };
            }
            Some(schema) => schema,
        };

        // An explicit JSON `null` payload means the same thing as an absent one.
        // The REST body types it `Option<Value>` (serde maps `null` -> `None`)
        // while the MCP argument map hands us `Some(Value::Null)`; without this
        // normalization the two receipt paths disagree on the same wire payload.
        let payload = payload.filter(|v| !v.is_null());

        let obj = match payload {
            Some(Value::Object(obj)) => obj,
            None => {
                // An absent payload still satisfies a kind with no required
                // fields; otherwise name the first missing required field.
                return match schema.iter().find(|spec| spec.required) {
                    Some(spec) => Err(format!(
                        "payload for kind '{}' missing required field '{}'",
                        self.as_str(),
                        spec.name
                    )),
                    None => Ok(()),
                };
            }
            Some(_) => {
                return Err(format!(
                    "payload for kind '{}' must be a JSON object",
                    self.as_str()
                ))
            }
        };

        for spec in schema {
            let value = match obj.get(spec.name) {
                Some(Value::Null) | None => {
                    // JSON null is treated as missing: a required field is
                    // never satisfiable by null; an optional one may be.
                    if spec.required {
                        return Err(format!(
                            "payload for kind '{}' missing required field '{}'",
                            self.as_str(),
                            spec.name
                        ));
                    }
                    continue;
                }
                Some(v) => v,
            };
            if let Err(expected) = Self::check_field_type(spec.ty, value) {
                return Err(format!(
                    "payload for kind '{}' field '{}' must be {}",
                    self.as_str(),
                    spec.name,
                    expected
                ));
            }
        }
        Ok(())
    }
