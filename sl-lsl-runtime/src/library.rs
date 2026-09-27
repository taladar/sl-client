//! The LSL library: one generated table, typed implementations, an erased
//! dispatch and a coverage count.
//!
//! **The table** is generated at build time from the vendored
//! `keywords_lsl_default.xml` — Linden Lab's own `LSLSyntax` document as the
//! viewer ships it (provenance in the crate `README.md`). It yields one
//! [`BuiltinId`] and one [`Builtin`] descriptor per function (name, typed
//! parameters, return type, forced sleep, energy, flags, description), every
//! [`Constant`] with its typed value, and every [`Event`] with its
//! parameters. Everything that needs to know the library reads it: the
//! lowering resolves a call to a [`BuiltinId`], the dispatch checks
//! arguments against it, and the grid's own `LSLSyntax` document is to be
//! rendered from it.
//!
//! **Implementations** are ordinary typed functions in the tranche modules,
//! registered in `library/registry.rs`; see [`dispatch`] for how the generated
//! [`Signature`] holds each one to the table at compile time.
//!
//! **Coverage** — implemented, stubbed or missing, per function — is
//! [`status`], and a test prints the counts and fails if a function falls
//! back from the committed baseline (`library/coverage.txt`).

pub mod control;
pub mod detection;
pub mod dispatch;
mod generated;
pub mod lists;
pub mod math;
mod registry;
pub mod table;

pub use dispatch::{
    CallError, Called, FromArgs, FromValue, Handler, IntoReturn, Key, Signature, Status, invoke,
    stub,
};
pub use generated::{BUILTINS, BuiltinId, CONSTANTS, EVENTS, signatures};
pub use registry::{call, status};
pub use table::{Argument, Builtin, Constant, ConstantValue, Event, builtin, constant, event};

#[cfg(test)]
mod tests {
    #![expect(
        clippy::print_stderr,
        reason = "the coverage test reports the library's progress to the operator"
    )]

    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;
    use sl_lsl::ast::TypeName;
    use sl_types::lsl::{Rotation, Vector};

    use super::*;
    use crate::value::{Element, Value};
    use crate::vm::{CallerId, Host, ScriptCtx, ScriptData, Tick};

    /// A host for calls that never reach it: the dispatch reports stubs
    /// through `Called::stubbed`, and only the VM tells the host.
    struct Unreached;

    impl Host for Unreached {
        fn print(&mut self, _caller: CallerId, _text: &str) {}

        fn stubbed(&mut self, _caller: CallerId, _id: BuiltinId) {}

        fn left_state(&mut self, _caller: CallerId) {}
    }

    #[test]
    fn the_table_is_sorted_and_indexed_consistently() {
        assert_eq!(BUILTINS.len(), BuiltinId::ALL.len());
        for (index, id) in BuiltinId::ALL.iter().enumerate() {
            assert_eq!(id.index(), index);
            assert_eq!(id.descriptor().id, *id);
        }
        assert!(BUILTINS.windows(2).all(|pair| match pair {
            [a, b] => a.name < b.name,
            _ => true,
        }));
        assert!(CONSTANTS.windows(2).all(|pair| match pair {
            [a, b] => a.name < b.name,
            _ => true,
        }));
        assert!(EVENTS.windows(2).all(|pair| match pair {
            [a, b] => a.name < b.name,
            _ => true,
        }));
    }

    #[test]
    fn lookups_by_name() {
        assert_eq!(
            builtin("llSetPos").map(|b| (b.args.len(), b.ret, b.sleep.to_bits())),
            Some((1, None, 0.2_f32.to_bits()))
        );
        assert_eq!(
            builtin("llAbs").map(|b| (b.args, b.ret)),
            Some((
                &[Argument {
                    name: "Value",
                    ty: TypeName::Integer
                }][..],
                Some(TypeName::Integer)
            ))
        );
        assert_eq!(builtin("llNoSuchFunction"), None);
        assert_eq!(event("touch_start").map(|e| e.args.len()), Some(1));
        assert_eq!(event("listen").map(|e| e.args.len()), Some(4));
        assert!(builtin("llMakeFire").is_some_and(|b| b.deprecated));
    }

    #[test]
    fn constants_decode_the_documents_notations() {
        let value = |name: &str| constant(name).map(|c| c.value.to_value());
        assert_eq!(value("TRUE"), Some(Value::Integer(1)));
        assert_eq!(value("CHANGED_INVENTORY"), Some(Value::Integer(1)));
        assert_eq!(value("PI"), Some(Value::Float(core::f32::consts::PI)));
        // `NULL_KEY` is a string in LSL, served as a UUID member.
        assert_eq!(
            value("NULL_KEY"),
            Some(Value::String(crate::value::NULL_KEY.to_owned()))
        );
        // `EOF` is served escaped twice; the `JSON_*` markers as `U+XXXX`.
        assert_eq!(value("EOF"), Some(Value::String("\n\n\n".to_owned())));
        assert_eq!(
            value("JSON_ARRAY"),
            Some(Value::String("\u{FDD2}".to_owned()))
        );
        assert_eq!(
            value("TOUCH_INVALID_TEXCOORD"),
            Some(Value::Vector(Vector {
                x: -1.0,
                y: -1.0,
                z: 0.0,
            }))
        );
        assert_eq!(
            value("ZERO_ROTATION"),
            Some(Value::Rotation(Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            }))
        );
        // `default` is listed among the constants for highlighting only.
        assert_eq!(constant("default"), None);
    }

    /// The vendored document, as `sl-wire`'s independent `LSLSyntax`
    /// decoder reads it.
    fn decoded() -> Result<sl_lsl::LslSyntax, String> {
        let source = include_str!("../keywords_lsl_default.xml");
        let document = sl_wire::parse_llsd_xml(source).map_err(|error| error.to_string())?;
        sl_wire::parse_lsl_syntax(&document).map_err(|error| error.to_string())
    }

    #[test]
    fn the_generated_table_matches_an_independent_decoder() -> Result<(), String> {
        let syntax = decoded()?;
        assert_eq!(syntax.functions.len(), BUILTINS.len());
        for descriptor in &BUILTINS {
            let function = syntax
                .functions
                .get(descriptor.name)
                .ok_or_else(|| format!("{} missing", descriptor.name))?;
            let types: Vec<Option<TypeName>> =
                function.arguments.iter().map(|a| a.arg_type).collect();
            let ours: Vec<Option<TypeName>> = descriptor.args.iter().map(|a| Some(a.ty)).collect();
            assert_eq!(ours, types, "{}", descriptor.name);
            assert_eq!(descriptor.ret, function.return_type, "{}", descriptor.name);
            assert_eq!(
                Some(descriptor.sleep.to_bits()),
                function.sleep.map(f32::to_bits),
                "{}",
                descriptor.name
            );
            assert_eq!(
                Some(descriptor.energy.to_bits()),
                function.energy.map(f32::to_bits),
                "{}",
                descriptor.name
            );
            assert_eq!(
                descriptor.deprecated, function.deprecated,
                "{}",
                descriptor.name
            );
            assert_eq!(
                descriptor.god_mode, function.god_mode,
                "{}",
                descriptor.name
            );
        }
        assert_eq!(syntax.events.len(), EVENTS.len());
        for event in &EVENTS {
            let decoded = syntax.events.get(event.name).ok_or(event.name)?;
            let names: Vec<&str> = decoded.arguments.iter().map(|a| a.name.as_str()).collect();
            let ours: Vec<&str> = event.args.iter().map(|a| a.name).collect();
            assert_eq!(ours, names, "{}", event.name);
        }
        // Every constant but the `default` placeholder, with the served text
        // cast by the runtime's own LSL parsers — except the two string
        // notations the table decodes.
        assert_eq!(syntax.constants.len(), CONSTANTS.len().saturating_add(1));
        for constant in &CONSTANTS {
            let decoded = syntax.constants.get(constant.name).ok_or(constant.name)?;
            assert_eq!(
                decoded.constant_type,
                Some(constant.value.type_name()),
                "{}",
                constant.name
            );
            let text = decoded.value.clone().unwrap_or_default();
            let notation = constant.name == "EOF" || text.starts_with("U+");
            if !notation {
                let parsed = crate::cast(Value::String(text), constant.value.type_name())
                    .map_err(|error| error.to_string())?;
                assert_eq!(parsed, constant.value.to_value(), "{}", constant.name);
            }
        }
        Ok(())
    }

    #[test]
    fn calls_are_checked_against_the_table() {
        let mut host = Unreached;
        let mut data = ScriptData::default();
        let mut ctx = ScriptCtx::new(
            CallerId(1),
            Tick(0),
            core::time::Duration::from_millis(100),
            &mut host,
            &mut data,
        );
        assert_eq!(
            call(BuiltinId::LlAbs, &mut ctx, vec![Value::Integer(-3)]),
            Ok(Called {
                value: Some(Value::Integer(3)),
                stubbed: false,
            })
        );
        assert_eq!(
            call(
                BuiltinId::LlGetListLength,
                &mut ctx,
                vec![Value::List(vec![Element::Integer(1)])]
            ),
            Ok(Called {
                value: Some(Value::Integer(1)),
                stubbed: false,
            })
        );
        assert_eq!(
            call(BuiltinId::LlAbs, &mut ctx, vec![]),
            Err(CallError::Arity {
                id: BuiltinId::LlAbs,
                expected: 1,
                found: 0,
            })
        );
        assert_eq!(
            call(BuiltinId::LlAbs, &mut ctx, vec![Value::Float(1.0)]),
            Err(CallError::Argument {
                id: BuiltinId::LlAbs,
                index: 0,
                expected: TypeName::Integer,
                found: TypeName::Float,
            })
        );
        assert_eq!(
            call(BuiltinId::LlSay, &mut ctx, vec![]),
            Err(CallError::Missing(BuiltinId::LlSay))
        );
    }

    /// A registry of its own, to exercise the stub path.
    #[allow(
        unreachable_pub,
        clippy::allow_attributes,
        reason = "the macro declares `pub` items for the crate's registry"
    )]
    mod with_stubs {
        crate::registry! {
            implemented { LlAbs => crate::library::math::ll_abs }
            stubbed { LlGetPos, LlSay }
        }
    }

    #[test]
    fn a_stub_checks_its_arguments_and_returns_the_default() {
        let mut host = Unreached;
        let mut data = ScriptData::default();
        let mut ctx = ScriptCtx::new(
            CallerId(1),
            Tick(0),
            core::time::Duration::from_millis(100),
            &mut host,
            &mut data,
        );
        assert_eq!(
            with_stubs::call(BuiltinId::LlGetPos, &mut ctx, vec![]),
            Ok(Called {
                value: Some(Value::Vector(crate::value::ZERO_VECTOR)),
                stubbed: true,
            })
        );
        assert_eq!(
            with_stubs::call(
                BuiltinId::LlSay,
                &mut ctx,
                vec![Value::Integer(0), Value::String("hi".to_owned())]
            ),
            Ok(Called {
                value: None,
                stubbed: true,
            })
        );
        assert_eq!(
            with_stubs::call(BuiltinId::LlSay, &mut ctx, vec![Value::Integer(0)]),
            Err(CallError::Arity {
                id: BuiltinId::LlSay,
                expected: 2,
                found: 1,
            })
        );
        assert_eq!(with_stubs::status(BuiltinId::LlSay), Status::Stubbed);
        assert_eq!(with_stubs::status(BuiltinId::LlAbs), Status::Implemented);
        assert_eq!(with_stubs::status(BuiltinId::LlFabs), Status::Missing);
    }

    /// The committed baseline: function name to the status it must not fall
    /// below.
    fn baseline() -> Result<BTreeMap<&'static str, Status>, String> {
        let mut entries = BTreeMap::new();
        for line in include_str!("library/coverage.txt").lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (status, name) = line
                .split_once(' ')
                .ok_or_else(|| format!("coverage.txt: `{line}`"))?;
            let status = match status {
                "implemented" => Status::Implemented,
                "stubbed" => Status::Stubbed,
                other => return Err(format!("coverage.txt: status `{other}`")),
            };
            if builtin(name).is_none() {
                return Err(format!("coverage.txt: no function `{name}`"));
            }
            let _previous = entries.insert(name, status);
        }
        Ok(entries)
    }

    #[test]
    fn coverage_never_regresses() -> Result<(), String> {
        let baseline = baseline()?;
        let mut counts: BTreeMap<Status, usize> = BTreeMap::new();
        let mut regressed = Vec::new();
        let mut unlisted = Vec::new();
        for descriptor in &BUILTINS {
            let now = status(descriptor.id);
            let entry = counts.entry(now).or_default();
            *entry = entry.saturating_add(1);
            let expected = baseline
                .get(descriptor.name)
                .copied()
                .unwrap_or(Status::Missing);
            if now < expected {
                regressed.push(format!("{} ({expected:?} → {now:?})", descriptor.name));
            } else if now > expected {
                unlisted.push(format!("{now:?} {}", descriptor.name));
            }
        }
        let count = |status| counts.get(&status).copied().unwrap_or(0);
        eprintln!(
            "LSL library coverage: {} implemented / {} stubbed / {} missing of {}",
            count(Status::Implemented),
            count(Status::Stubbed),
            count(Status::Missing),
            BUILTINS.len()
        );
        if !unlisted.is_empty() {
            eprintln!(
                "not yet in coverage.txt (add them): {}",
                unlisted.join(", ")
            );
        }
        if regressed.is_empty() {
            Ok(())
        } else {
            Err(format!("coverage regressed: {}", regressed.join(", ")))
        }
    }
}
