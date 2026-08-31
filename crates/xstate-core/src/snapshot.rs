use serde::{Deserialize, Serialize};

pub struct Snapshot<C, S> {
    pub state_value: S,
    pub context: C,
    pub history: Vec<S>,
    pub output: Option<serde_json::Value>,
}

impl<C, S> Snapshot<C, S>
where
    C: Serialize,
    S: Serialize,
{
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

impl<C, S> Serialize for Snapshot<C, S>
where
    C: Serialize,
    S: Serialize,
{
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Snapshot", 4)?;
        s.serialize_field("state_value", &self.state_value)?;
        s.serialize_field("context", &self.context)?;
        s.serialize_field("history", &self.history)?;
        s.serialize_field("output", &self.output)?;
        s.end()
    }
}

impl<'de, C, S> Deserialize<'de> for Snapshot<C, S>
where
    C: Deserialize<'de>,
    S: Deserialize<'de>,
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Helper<C, S> {
            state_value: S,
            context: C,
            history: Vec<S>,
            #[serde(default)]
            output: Option<serde_json::Value>,
        }
        let h = Helper::deserialize(deserializer)?;
        Ok(Snapshot {
            state_value: h.state_value,
            context: h.context,
            history: h.history,
            output: h.output,
        })
    }
}
