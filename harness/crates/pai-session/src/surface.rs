//! Surface: phần sổ mà mô hình nhìn thấy.
//!
//! Sổ là chỉ-ghi-thêm, nên nén ngữ cảnh không được phép xoá. Thay vào đó nó ghi thêm một
//! sự kiện mang thao tác `replace(start, end)` che một dải node cũ. Bản ghi vẫn đủ để
//! phát lại nguyên vẹn; chỉ **phép chiếu** thấy dải đó biến mất.
//!
//! `start`/`end` là **vị trí trong danh sách node surface**, không phải `seq`. Sau một
//! lần replace, thứ tự node đã đổi, nên đánh địa chỉ bằng seq sẽ trỏ vào chỗ khác với
//! thứ mô hình đang thấy. Danh sách node là sự thật; `source_event_seqs` là dấu vết.

use serde::de::{self, Deserializer};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use crate::error::{Result, SessionError};
use crate::event::Seq;

/// Node mới vào cuối, hay che một dải node đã có.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceOp {
    Append,
    /// Nửa mở: `start..end`. `start == end` là một phép chèn, hợp lệ nhưng vô nghĩa.
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

/// Thứ tự node theo đúng cách mô hình nhìn thấy.
#[derive(Clone, Debug, Default)]
pub struct Surface {
    nodes: Vec<Seq>,
    /// Tăng mỗi lần có `replace`. Bộ nhớ đệm của phép chiếu chỉ cần so con số này để biết
    /// nó phải dựng lại từ đầu hay chỉ gấp thêm phần đuôi.
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

    /// Seq của những node mà một `replace(start, end)` sẽ che.
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

    /// Gấp một sự kiện surface vào danh sách node.
    ///
    /// `sources` bắt buộc phải kê đủ node bị che: đó là dấu vết duy nhất còn lại để biết
    /// bản tóm tắt đã nuốt những gì. Kê thừa thì được — một lần nén có thể trích dẫn cả
    /// những sự kiện log-only mà nó đã đọc.
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
