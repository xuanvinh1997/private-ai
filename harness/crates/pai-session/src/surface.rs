//! Surface: the part of the log the model sees. Compaction never deletes; it appends a
//! `replace(start, end)` that shadows a range, so replay stays intact and only the projection changes.
//! `start`/`end` are positions in the node list, not seqs, because replace reorders nodes.

use serde::de::{self, Deserializer};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use crate::error::{Result, SessionError};
use crate::event::Seq;

/// Either a new node at the end, or a shadow over an existing range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceOp {
    Append,
    /// Half-open `start..end`; `start == end` is an insert -- legal but pointless.
    Replace {
        start: usize,
        end: usize,
    },
}

impl Serialize for SurfaceOp {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            SurfaceOp::Append => serializer.serialize_str("append"),
            SurfaceOp::Replace { start, end } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("op", "replace")?;
                map.serialize_entry("start", start)?;
                map.serialize_entry("end", end)?;
                map.end()
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawOp {
    Append(String),
    Replace {
        op: String,
        start: usize,
        end: usize,
    },
}

impl<'de> Deserialize<'de> for SurfaceOp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<SurfaceOp, D::Error> {
        match RawOp::deserialize(d)? {
            RawOp::Append(tag) if tag == "append" => Ok(SurfaceOp::Append),
            RawOp::Append(tag) => Err(de::Error::custom(format!("surface_op lạ: `{tag}`"))),
            RawOp::Replace { op, start, end } if op == "replace" => {
                Ok(SurfaceOp::Replace { start, end })
            }
            RawOp::Replace { op, .. } => Err(de::Error::custom(format!("surface_op lạ: `{op}`"))),
        }
    }
}

/// Node order exactly as the model sees it.
#[derive(Clone, Debug, Default)]
pub struct Surface {
    nodes: Vec<Seq>,
    /// Bumped on every `replace`; the projection cache compares it to decide rebuild versus fold-the-tail.
    generation: u64,
}

impl Surface {
    pub fn nodes(&self) -> &[Seq] {
        &self.nodes
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Seqs of the nodes a `replace(start, end)` would shadow.
    pub fn shadowed(&self, start: usize, end: usize) -> Result<Vec<Seq>> {
        if start > end || end > self.nodes.len() {
            return Err(SessionError::SurfaceRangeOutOfBounds {
                start,
                end,
                len: self.nodes.len(),
            });
        }
        Ok(self.nodes[start..end].to_vec())
    }

    /// Fold a surface event into the node list. `sources` must cite every shadowed node -- the only
    /// remaining trace of what the summary swallowed -- but may cite more, such as log-only events read.
    pub fn apply(&mut self, seq: Seq, op: SurfaceOp, sources: Option<&[Seq]>) -> Result<()> {
        match op {
            SurfaceOp::Append => self.nodes.push(seq),
            SurfaceOp::Replace { start, end } => {
                let shadowed = self.shadowed(start, end)?;
                let cited = sources.unwrap_or(&[]);
                let missing: Vec<Seq> = shadowed
                    .iter()
                    .copied()
                    .filter(|s| !cited.contains(s))
                    .collect();
                if !missing.is_empty() {
                    return Err(SessionError::UncitedShadow { missing });
                }
                self.nodes.splice(start..end, [seq]);
                self.generation += 1;
            }
        }
        Ok(())
    }
}
