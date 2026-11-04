//! Plate layout: where parts go when you press "arrange".
//!
//! The rule is the one every slicer uses — biggest footprint first, fill a shelf
//! left to right, start a new shelf when the next part does not fit — because it
//! is cheap, predictable, and keeps parts of a kind near each other. Parts that
//! are wider than the plate still get a shelf of their own and are centred on it,
//! so they hang off both edges rather than off one.

/// Centre points for each footprint, in the same order as `sizes`.
///
/// `sizes` are `(width, depth)` in millimetres; the returned centres are in bed
/// coordinates with the plate's near-left corner at `(0, 0)`. Parts are packed
/// largest-first; the returned order still matches the input.
pub fn shelves(sizes: &[(f64, f64)], width: f64, depth: f64, gap: f64) -> Vec<[f64; 2]> {
    let mut slots = vec![[0.0, 0.0]; sizes.len()];
    if sizes.is_empty() {
        return slots;
    }
    let usable = (width - gap * 2.0).max(0.0);

    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|left, right| {
        let a = sizes[*left].0.max(sizes[*left].1);
        let b = sizes[*right].0.max(sizes[*right].1);
        b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut cursor_x = gap;
    let mut cursor_y = gap;
    let mut shelf_depth = 0.0_f64;

    for index in order {
        let (w, d) = sizes[index];
        if cursor_x + w > gap + usable && cursor_x > gap {
            cursor_x = gap;
            cursor_y += shelf_depth + gap;
            shelf_depth = 0.0;
        }
        // A part wider than the plate is centred rather than pushed off the right.
        let centre_x = if w > usable {
            width * 0.5
        } else {
            cursor_x + w * 0.5
        };
        slots[index] = [centre_x, cursor_y + d * 0.5];
        cursor_x += w + gap;
        shelf_depth = shelf_depth.max(d);
    }

    // If the shelves ran past the far edge, pull the whole block back on.
    let far = slots
        .iter()
        .enumerate()
        .map(|(index, slot)| slot[1] + sizes[index].1 * 0.5)
        .fold(f64::NEG_INFINITY, f64::max);
    let over = far - (depth - gap);
    if over > 0.0 {
        for slot in &mut slots {
            slot[1] -= over;
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::shelves;

    #[test]
    fn two_parts_sit_side_by_side_on_one_shelf() {
        let slots = shelves(&[(70.0, 13.0), (70.0, 13.0)], 256.0, 256.0, 4.0);
        assert_eq!(slots.len(), 2);
        // 4 mm margin, then 35 mm to the first centre; 70 + 4 mm gap to the next.
        assert!((slots[0][0] - 39.0).abs() < 1e-9, "{slots:?}");
        assert!((slots[1][0] - 113.0).abs() < 1e-9, "{slots:?}");
        assert!((slots[0][1] - 10.5).abs() < 1e-9, "{slots:?}");
        assert_eq!(slots[0][1], slots[1][1]);
    }

    #[test]
    fn the_biggest_part_leads_and_keeps_its_slot() {
        // Input order is small, big; the big one is placed first but both keep
        // their original index in the result.
        let slots = shelves(&[(40.0, 40.0), (120.0, 120.0)], 256.0, 256.0, 4.0);
        assert!((slots[1][0] - 64.0).abs() < 1e-9, "{slots:?}");
        assert!((slots[0][0] - 148.0).abs() < 1e-9, "{slots:?}");
    }

    #[test]
    fn a_part_wider_than_the_plate_is_centred() {
        let slots = shelves(&[(300.0, 10.0)], 256.0, 256.0, 4.0);
        assert!((slots[0][0] - 128.0).abs() < 1e-9, "{slots:?}");
    }

    #[test]
    fn a_full_plate_wraps_to_a_second_shelf() {
        let sizes = vec![(100.0, 100.0); 9];
        let slots = shelves(&sizes, 220.0, 220.0, 4.0);
        let rows: Vec<f64> = slots.iter().map(|slot| slot[1]).collect();
        let first_row = rows[0];
        assert!(rows.iter().any(|y| (*y - first_row).abs() > 1.0), "{rows:?}");
    }
}
