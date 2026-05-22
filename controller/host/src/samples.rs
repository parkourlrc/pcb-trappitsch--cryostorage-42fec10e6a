//! Module to handle sample names and locations.
use anyhow::{Result, bail};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const SMP_POSITIONS: [&str; 8] = ["A1", "A2", "B1", "B2", "C1", "C2", "D1", "D2"];
const SMP_BUTTON_WIDTH: f32 = 272.5;
const SMP_BUTTON_HEIGHT: f32 = 200.;

/// Contain all the information about a sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    /// The name of the sample.
    name: String,
    /// Timestamp when the sample was first added to the list. None if empty sample.
    timestamp_added: Option<DateTime<Local>>,
}

impl Sample {
    /// Create an empty sample and return it.
    pub fn empty() -> Sample {
        Sample {
            name: "".into(),
            timestamp_added: None,
        }
    }

    /// Create a new sample from a given name with current timestamp.
    pub fn new(name: &str) -> Sample {
        Sample {
            name: name.into(),
            timestamp_added: Some(Local::now()),
        }
    }

    /// Get the date as a fomratted string or an empty string if no date.
    pub fn get_date(&self) -> String {
        if let Some(dt) = self.timestamp_added {
            dt.format("%Y-%m-%d").to_string()
        } else {
            String::new()
        }
    }

    /// Get only the sample name
    pub fn get_name(&self) -> String {
        self.name.clone()
    }
}

/// Transform &str into a Sample.
///
/// If the &str is empty, return an empty sample w/o a timestamp.
impl From<&str> for Sample {
    fn from(value: &str) -> Self {
        if value.trim().is_empty() {
            Self::empty()
        } else {
            Self::new(value)
        }
    }
}

/// Hold all samples and their loaded positions.
///
/// This stores a BTreeMap where the key is the position (unique) and the value the sample itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Samples {
    names: BTreeMap<String, Sample>,
}

impl Samples {
    /// Create a new Samples instance with empty names for each position.
    pub fn new() -> Self {
        let mut names = BTreeMap::new();
        for &pos in SMP_POSITIONS.iter() {
            names.insert(pos.into(), Sample::empty());
        }
        Self { names }
    }

    /// Execute swipe action.
    ///
    /// If the swipe swap is invalid (out of bounds, drop on same as want to swap, ...), this
    /// function returns None. It's not an error, we just don't want to act on it.
    pub fn execute_swipe_swap(
        &mut self,
        pos1: &str,
        dx: f32,
        dy: f32,
    ) -> Option<[(String, Sample); 2]> {
        let pos2 = sample_button_swipe(pos1, dx, dy).ok()?;

        if pos1 != pos2 {
            self.swap_positions(pos1, &pos2)
        } else {
            None
        }
    }

    /// Is the samples list empty?
    pub fn is_empty(&self) -> bool {
        let cnt = self.names.iter().filter(|s| !s.1.name.is_empty()).count();
        cnt == 0
    }

    /// Swap two positions.
    ///
    /// Returns an array that contains two tuples, each touple containing the a position name and a
    /// sample, these are the new positions.
    fn swap_positions(&mut self, pos1: &str, pos2: &str) -> Option<[(String, Sample); 2]> {
        let smp1 = self.names.get(pos1)?.clone();
        let smp2 = self.names.get(pos2)?.clone();

        self.names.insert(pos1.into(), smp2.clone());
        self.names.insert(pos2.into(), smp1.clone());

        Some([(pos1.into(), smp2), (pos2.into(), smp1)])
    }

    /// Update the sample name at the given position and return the index of the entry.
    pub fn update_sample(&mut self, pos: &str, value: &str) -> Result<Sample> {
        if self.names.contains_key(pos) {
            let smp: Sample = value.into();

            self.names.insert(pos.into(), smp.clone());

            Ok(smp)
        } else {
            bail!("Position {} does not exist", pos);
        }
    }
}

// Implement Iterator trait for owned value of Samples.
//
// It returns a tuple of (position, sample_name) for each iteration, until depleted.
// Position come from SMP_POSITIONS, and sample_name from the BTreeMap.
impl IntoIterator for Samples {
    type Item = (String, Sample);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        let mut items = Vec::new();
        for &pos in SMP_POSITIONS.iter() {
            if let Some(sample) = self.names.get(pos) {
                items.push((pos.into(), sample.clone()));
            }
        }
        items.into_iter()
    }
}

/// Calculation for sample button swipe.
///
/// If the move cannot be completed, this will just fail with the respective error.
fn sample_button_swipe(pos: &str, dx: f32, dy: f32) -> Result<String> {
    // calculate delta row and col
    let drow = (dy / SMP_BUTTON_HEIGHT).round() as isize;
    let dcol = (dx / SMP_BUTTON_WIDTH).round() as isize;

    // current row and col
    let (mut row, mut col) = position_name_to_row_col(pos)?;

    // new row and col plus new position
    row += drow;
    col += dcol;
    let new_pos = row_col_to_position_name(row, col)?;

    Ok(new_pos)
}

/// Function to transform sample name to row, col as displayed in GUI
fn position_name_to_row_col(value: &str) -> Result<(isize, isize)> {
    match value {
        "A1" => Ok((0, 0)),
        "A2" => Ok((1, 0)),
        "B1" => Ok((0, 1)),
        "B2" => Ok((1, 1)),
        "C1" => Ok((0, 2)),
        "C2" => Ok((1, 2)),
        "D1" => Ok((0, 3)),
        "D2" => Ok((1, 3)),
        _ => bail!("Position {value} unknown"),
    }
}

/// Function to transform row, col to position name.
fn row_col_to_position_name(row: isize, col: isize) -> Result<String> {
    match (row, col) {
        (0, 0) => Ok("A1".into()),
        (1, 0) => Ok("A2".into()),
        (0, 1) => Ok("B1".into()),
        (1, 1) => Ok("B2".into()),
        (0, 2) => Ok("C1".into()),
        (1, 2) => Ok("C2".into()),
        (0, 3) => Ok("D1".into()),
        (1, 3) => Ok("D2".into()),
        _ => bail!("Row {row} and/or col {col} out of range"),
    }
}

/// Get the index number for a sample position given its name.
pub fn get_sample_idx(pos: &str) -> Option<usize> {
    SMP_POSITIONS.iter().position(|&p| p == pos)
}

// tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let smp = Samples::new();
        assert_eq!(smp.names.len(), 8);
    }
}
