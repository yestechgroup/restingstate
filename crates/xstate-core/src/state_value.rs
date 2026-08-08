use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserializer, Serializer};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StateValue {
    Atomic(String),
    Compound {
        state: String,
        child: Box<StateValue>,
    },
    Parallel {
        regions: Vec<(String, StateValue)>,
    },
}

impl serde::Serialize for StateValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            StateValue::Atomic(name) => serializer.serialize_str(name),
            StateValue::Compound { state, child } => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(state, child)?;
                map.end()
            }
            StateValue::Parallel { regions } => {
                let mut map = serializer.serialize_map(Some(regions.len()))?;
                for (key, val) in regions {
                    map.serialize_entry(key, val)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for StateValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StateValueVisitor;

        impl<'de> Visitor<'de> for StateValueVisitor {
            type Value = StateValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string or an object representing a state value")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StateValue::Atomic(v.to_string()))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StateValue::Atomic(v))
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut entries: Vec<(String, StateValue)> = Vec::new();
                while let Some((key, value)) = map.next_entry()? {
                    entries.push((key, value));
                }

                if entries.is_empty() {
                    return Err(de::Error::invalid_length(0, &self));
                }

                if entries.len() == 1 {
                    let (state, child) = entries.into_iter().next().unwrap();
                    Ok(StateValue::Compound {
                        state,
                        child: Box::new(child),
                    })
                } else {
                    Ok(StateValue::Parallel { regions: entries })
                }
            }
        }

        deserializer.deserialize_any(StateValueVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_serialize() {
        let val = StateValue::Atomic("green".into());
        let json = serde_json::to_string(&val).unwrap();
        assert_eq!(json, r#""green""#);
    }

    #[test]
    fn atomic_deserialize() {
        let val: StateValue = serde_json::from_str(r#""green""#).unwrap();
        assert_eq!(val, StateValue::Atomic("green".into()));
    }

    #[test]
    fn compound_serialize() {
        let val = StateValue::Compound {
            state: "editing".into(),
            child: Box::new(StateValue::Atomic("filled".into())),
        };
        let json = serde_json::to_string(&val).unwrap();
        assert_eq!(json, r#"{"editing":"filled"}"#);
    }

    #[test]
    fn compound_deserialize() {
        let val: StateValue = serde_json::from_str(r#"{"editing":"filled"}"#).unwrap();
        assert_eq!(
            val,
            StateValue::Compound {
                state: "editing".into(),
                child: Box::new(StateValue::Atomic("filled".into())),
            }
        );
    }

    #[test]
    fn compound_nested_serialize() {
        let val = StateValue::Compound {
            state: "a".into(),
            child: Box::new(StateValue::Compound {
                state: "b".into(),
                child: Box::new(StateValue::Atomic("c".into())),
            }),
        };
        let json = serde_json::to_string(&val).unwrap();
        assert_eq!(json, r#"{"a":{"b":"c"}}"#);
    }

    #[test]
    fn compound_nested_deserialize() {
        let val: StateValue = serde_json::from_str(r#"{"a":{"b":"c"}}"#).unwrap();
        assert_eq!(
            val,
            StateValue::Compound {
                state: "a".into(),
                child: Box::new(StateValue::Compound {
                    state: "b".into(),
                    child: Box::new(StateValue::Atomic("c".into())),
                }),
            }
        );
    }

    #[test]
    fn compound_single_region_deserialized_as_compound() {
        let val: StateValue = serde_json::from_str(r#"{"fetching":"fetching"}"#).unwrap();
        assert_eq!(
            val,
            StateValue::Compound {
                state: "fetching".into(),
                child: Box::new(StateValue::Atomic("fetching".into())),
            }
        );
    }

    #[test]
    fn parallel_serialize() {
        let val = StateValue::Parallel {
            regions: vec![
                ("A".into(), StateValue::Atomic("a1".into())),
                ("B".into(), StateValue::Atomic("b1".into())),
            ],
        };
        let json = serde_json::to_string(&val).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, serde_json::json!({"A": "a1", "B": "b1"}));
    }

    #[test]
    fn parallel_deserialize() {
        let val: StateValue = serde_json::from_str(r#"{"A":"a1","B":"b1"}"#).unwrap();
        assert_eq!(
            val,
            StateValue::Parallel {
                regions: vec![
                    ("A".into(), StateValue::Atomic("a1".into())),
                    ("B".into(), StateValue::Atomic("b1".into())),
                ],
            }
        );
    }

    #[test]
    fn roundtrip_atomic() {
        let val = StateValue::Atomic("green".into());
        let json = serde_json::to_string(&val).unwrap();
        let back: StateValue = serde_json::from_str(&json).unwrap();
        assert_eq!(val, back);
    }

    #[test]
    fn roundtrip_compound() {
        let val = StateValue::Compound {
            state: "editing".into(),
            child: Box::new(StateValue::Atomic("filled".into())),
        };
        let json = serde_json::to_string(&val).unwrap();
        let back: StateValue = serde_json::from_str(&json).unwrap();
        assert_eq!(val, back);
    }

    #[test]
    fn roundtrip_nested_compound() {
        let val = StateValue::Compound {
            state: "a".into(),
            child: Box::new(StateValue::Compound {
                state: "b".into(),
                child: Box::new(StateValue::Atomic("c".into())),
            }),
        };
        let json = serde_json::to_string(&val).unwrap();
        let back: StateValue = serde_json::from_str(&json).unwrap();
        assert_eq!(val, back);
    }

    #[test]
    fn roundtrip_parallel() {
        let val = StateValue::Parallel {
            regions: vec![
                ("A".into(), StateValue::Atomic("a1".into())),
                ("B".into(), StateValue::Atomic("b1".into())),
            ],
        };
        let json = serde_json::to_string(&val).unwrap();
        let back: StateValue = serde_json::from_str(&json).unwrap();
        assert_eq!(val, back);
    }
}
