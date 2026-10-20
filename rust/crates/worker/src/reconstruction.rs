//! Versioned reconstruction records. No dependency on report/pseudocode parsing.
use std::{collections::BTreeMap, io};

pub const PASS: &str = "arm64-reconstruction-3";
pub const ENGINE: &str = "arm64-analysis-v89-schema122";

pub struct Input<'a> {
    pub code: &'a [u8],
    pub base: u64,
    pub start: u64,
    pub end: u64,
    pub file_offset: u64,
    pub data_ranges: &'a [(u64, u64, u16)],
    pub source: &'a str,
    pub architecture: &'a str,
    pub name: Option<&'a str>,
    pub boundary_sources: Vec<String>,
    pub signatures: Vec<(String, crate::signatures::Signature)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i128),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}
impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}
impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Self::Int(i128::from(value))
    }
}
impl From<u8> for Value {
    fn from(value: u8) -> Self {
        Self::Int(i128::from(value))
    }
}
impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

pub fn object<const N: usize>(values: [(&str, Value); N]) -> Value {
    Value::Object(
        values
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect(),
    )
}
pub fn array(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(values.into_iter().collect())
}
pub fn address(value: u64) -> Value {
    format!("{value:#x}").into()
}
pub fn ranges(start: u64, end: u64) -> Value {
    array([object([("start", address(start)), ("end", address(end))])])
}

impl Value {
    pub fn json(&self) -> String {
        match self {
            Self::Null => "null".into(),
            Self::Bool(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::String(value) => super::quote(value),
            Self::Array(values) => format!(
                "[{}]",
                values.iter().map(Self::json).collect::<Vec<_>>().join(",")
            ),
            Self::Object(values) => format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!("{}:{}", super::quote(key), value.json()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
    pub fn insert(&mut self, key: &str, value: Value) {
        if let Self::Object(values) = self {
            values.insert(key.into(), value);
        }
    }
    pub fn get(&self, key: &str) -> Option<&Self> {
        if let Self::Object(values) = self {
            values.get(key)
        } else {
            None
        }
    }
    /// Same typed/length-framed encoding as scripts/reconstruction_artifact.py.
    pub fn canonical(&self) -> Vec<u8> {
        fn frame(output: &mut Vec<u8>, bytes: &[u8]) {
            output.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
            output.extend_from_slice(bytes);
        }
        fn encode(value: &Value, output: &mut Vec<u8>) {
            match value {
                Value::Null => output.push(b'n'),
                Value::Bool(value) => output.push(if *value { b't' } else { b'f' }),
                Value::Int(value) => {
                    output.push(b'i');
                    frame(output, value.to_string().as_bytes());
                }
                Value::String(value) => {
                    output.push(b's');
                    frame(output, value.as_bytes());
                }
                Value::Array(values) => {
                    output.push(b'a');
                    output.extend_from_slice(&(values.len() as u64).to_be_bytes());
                    for value in values {
                        encode(value, output);
                    }
                }
                Value::Object(values) => {
                    output.push(b'o');
                    output.extend_from_slice(&(values.len() as u64).to_be_bytes());
                    for (key, value) in values {
                        encode(&Value::String(key.clone()), output);
                        encode(value, output);
                    }
                }
            }
        }
        let mut result = Vec::new();
        encode(self, &mut result);
        result
    }
}

extern "C" {
    fn re_sha256_bytes(data: *const u8, size: usize, digest: *mut u8) -> i32;
}
pub fn digest(value: &Value) -> io::Result<String> {
    let bytes = value.canonical();
    // The provider writes 64 hexadecimal ASCII bytes. The buffer is a byte
    // buffer rather than a `c_char` buffer because `c_char` is signed on some
    // targets and unsigned on others, which would make the byte-to-character
    // conversion a no-op on one platform and a real conversion on another.
    let mut hash = [0u8; 65];
    if unsafe { re_sha256_bytes(bytes.as_ptr(), bytes.len(), hash.as_mut_ptr()) } != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Artifact SHA-256 provider failed or byte budget exceeded",
        ));
    }
    Ok(hash[..64].iter().map(|byte| *byte as char).collect())
}

pub struct Builder {
    source: String,
    pub scope: Value,
    pub function: Value,
    function_id: String,
    pub nodes: BTreeMap<String, Value>,
    pub edges: BTreeMap<String, Value>,
    pub ast: BTreeMap<String, Value>,
}
impl Builder {
    pub fn new(
        source: &str,
        start: u64,
        end: u64,
        architecture: &str,
        name: Option<&str>,
        settings: Value,
    ) -> io::Result<Self> {
        let scope = object([
            ("pass_version", PASS.into()),
            (
                "settings_digest",
                format!(
                    "sha256:{}",
                    digest(&array(["hindsight-settings-v1".into(), settings]))?
                )
                .into(),
            ),
        ]);
        let mut result = Self {
            source: source.into(),
            scope,
            function: Value::Null,
            function_id: String::new(),
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            ast: BTreeMap::new(),
        };
        let mut function = object([
            ("start", address(start)),
            ("end", address(end)),
            ("architecture", architecture.into()),
            ("abi", "apple-aapcs64-lp64".into()),
            ("name", name.map(Value::from).unwrap_or(Value::Null)),
        ]);
        result.function_id = result.identity("function", &function, None)?;
        function.insert("id", result.function_id.clone().into());
        result.function = function;
        Ok(result)
    }
    fn identity(&self, domain: &str, record: &Value, function: Option<&str>) -> io::Result<String> {
        let mut body = record.clone();
        if let Value::Object(fields) = &mut body {
            fields.remove("id");
        }
        let scope = array([
            self.scope.get("pass_version").unwrap().clone(),
            self.scope.get("settings_digest").unwrap().clone(),
        ]);
        Ok(format!(
            "sha256:{}",
            digest(&array([
                "hindsight-content-v2".into(),
                domain.into(),
                self.source.clone().into(),
                function.map(Value::from).unwrap_or(Value::Null),
                scope,
                body
            ]))?
        ))
    }
    pub fn record(
        &mut self,
        kind: &str,
        epistemic: &str,
        payload: Value,
        ranges: Value,
    ) -> io::Result<String> {
        let mut record = object([
            ("kind", kind.into()),
            ("epistemic", epistemic.into()),
            ("producer", PASS.into()),
            ("confidence", 1u64.into()),
            ("payload", payload),
            ("ranges", ranges),
        ]);
        let id = self.identity("evidence", &record, Some(&self.function_id))?;
        record.insert("id", id.clone().into());
        self.nodes.insert(id.clone(), record);
        Ok(id)
    }
    pub fn edge(&mut self, source: &str, target: &str, relation: &str) -> io::Result<()> {
        let mut record = object([
            ("source", source.into()),
            ("target", target.into()),
            ("relation", relation.into()),
            ("producer", PASS.into()),
        ]);
        let id = self.identity("edge", &record, Some(&self.function_id))?;
        record.insert("id", id.clone().into());
        self.edges.insert(id, record);
        Ok(())
    }
    pub fn ast(
        &mut self,
        kind: &str,
        ty: Option<&str>,
        children: Vec<String>,
        evidence: &str,
        span: (u64, u64),
        attributes: Value,
    ) -> io::Result<String> {
        self.ast_spans(
            kind,
            ty,
            children,
            &[evidence.to_owned()],
            ranges(span.0, span.1),
            attributes,
        )
    }
    pub fn ast_spans(
        &mut self,
        kind: &str,
        ty: Option<&str>,
        children: Vec<String>,
        evidence: &[String],
        spans: Value,
        attributes: Value,
    ) -> io::Result<String> {
        let mut record = object([
            ("kind", kind.into()),
            ("type", ty.map(Value::from).unwrap_or(Value::Null)),
            ("children", array(children.into_iter().map(Value::from))),
            ("evidence", array(evidence.iter().cloned().map(Value::from))),
            ("ranges", spans),
            ("confidence", 1u64.into()),
            ("attributes", attributes),
        ]);
        let id = self.identity("ast", &record, Some(&self.function_id))?;
        record.insert("id", id.clone().into());
        self.ast.insert(id.clone(), record);
        Ok(id)
    }
    pub fn finish(
        self,
        root: String,
        rendering: String,
        instructions: u64,
        truncated: bool,
        exhausted: bool,
    ) -> Value {
        let unsupported = self
            .nodes
            .iter()
            .filter(|(_, node)| node.get("kind") == Some(&"unsupported-effect".into()))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut reasons = unsupported
            .iter()
            .take(125)
            .map(|id| {
                object([
                    ("code", "unsupported-effect".into()),
                    ("message", "unsupported effect blocks source export".into()),
                    ("blocking", true.into()),
                    ("evidence", array([id.clone().into()])),
                ])
            })
            .collect::<Vec<_>>();
        if truncated {
            reasons.push(object([
                ("code", "truncated".into()),
                ("message", "function analysis was truncated".into()),
                ("blocking", true.into()),
                ("evidence", array([])),
            ]));
        }
        if exhausted {
            reasons.push(object([
                ("code", "budget-exhausted".into()),
                ("message", "instruction budget exhausted".into()),
                ("blocking", true.into()),
                ("evidence", array([])),
            ]));
        }
        object([
            ("schema", 1u64.into()),
            ("producer", "hindsight-reconstruction-v1".into()),
            ("source_sha256", self.source.into()),
            ("analysis_engine", ENGINE.into()),
            ("scope", self.scope),
            ("function", self.function),
            (
                "eligibility",
                object([
                    (
                        "state",
                        if reasons.is_empty() {
                            "review-only"
                        } else {
                            "blocked"
                        }
                        .into(),
                    ),
                    ("reasons", array(reasons)),
                ]),
            ),
            ("nodes", array(self.nodes.into_values())),
            ("edges", array(self.edges.into_values())),
            (
                "ast",
                object([
                    ("language", "c17".into()),
                    ("root", root.into()),
                    ("nodes", array(self.ast.into_values())),
                    ("rendering", rendering.into()),
                    ("compilable", false.into()),
                ]),
            ),
            ("alternatives", array([])),
            (
                "budgets",
                object([
                    ("limits", object([("instructions", 128u64.into())])),
                    ("consumed", object([("instructions", instructions.into())])),
                    (
                        "exhausted",
                        array(if exhausted {
                            vec!["instructions".into()]
                        } else {
                            vec![]
                        }),
                    ),
                ]),
            ),
            ("validation", array([])),
            ("truncated", truncated.into()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_sha256_and_canonical_encoding_are_stable() {
        assert_eq!(
            digest(&Value::Null).unwrap(),
            "1b16b1df538ba12dc3f97edbb85caa7050d46c148134290feba80f8236c83db9"
        );
        let value = object([
            ("z", Value::Null),
            ("a", array([true.into(), Value::Int(-7), "é".into()])),
        ]);
        assert_eq!(value.json(), "{\"a\":[true,-7,\"é\"],\"z\":null}");
        assert_eq!(value.canonical()[0], b'o');
    }
}
