use eframe::egui::{self, Vec2};

/// Scroll distance, in points, that moves the selection by one option. A mouse wheel notch
/// scrolls 40 points in egui, so one notch is one step.
const STEP: f32 = 40.0;
/// How much of a step must be scrolled before it counts, so that float rounding in the
/// smoothed scroll delta never loses a notch.
const THRESHOLD: f32 = 30.0;

/// Adds `delta` (positive when the wheel is turned up) to `accumulated` and returns how many
/// whole steps that completes. Positive steps move towards the next option, negative towards
/// the previous one.
fn take_steps(accumulated: &mut f32, delta: f32) -> i32 {
    *accumulated += delta;
    let mut steps = 0;
    while accumulated.abs() >= THRESHOLD {
        let direction = accumulated.signum();
        *accumulated -= direction * STEP;
        steps -= direction as i32;
    }
    steps
}

/// Moves `selected` by `steps` within `count` options, stopping at either end. With no
/// current selection, the first step lands on the first or last option. Returns `None` when
/// nothing changes.
fn step_index(selected: Option<usize>, count: usize, steps: i32) -> Option<usize> {
    if count == 0 || steps == 0 {
        return None;
    }
    let start = match selected {
        Some(index) => index as i64,
        None if steps > 0 => -1,
        None => count as i64,
    };
    let target = (start + steps as i64).clamp(0, count as i64 - 1) as usize;
    (selected != Some(target)).then_some(target)
}

/// Lets the mouse wheel step through the options of a combo box while the pointer is over it
/// and its list is closed. `selected` is the current option's position, or `None` if the
/// current value is not one of the options. Returns the position to switch to, if it changed.
///
/// The scroll is consumed, so an enclosing scroll area does not move at the same time.
pub fn wheel_select<R>(
    ui: &egui::Ui,
    combo: &egui::InnerResponse<Option<R>>,
    selected: Option<usize>,
    count: usize,
) -> Option<usize> {
    let id = combo.response.id.with("wheel");
    let listening = combo.inner.is_none() && combo.response.hovered();
    let delta = if listening {
        ui.input(|i| i.smooth_scroll_delta.y)
    } else {
        0.0
    };

    let steps = ui.data_mut(|data| {
        let accumulated = data.get_temp_mut_or_default::<f32>(id);
        if delta == 0.0 {
            // A pause between wheel turns drops any leftover partial step.
            *accumulated = 0.0;
            0
        } else {
            take_steps(accumulated, delta)
        }
    });
    if delta != 0.0 {
        ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
    }
    step_index(selected, count, steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_steps(frames: &[f32]) -> i32 {
        let mut accumulated = 0.0;
        frames
            .iter()
            .map(|&delta| take_steps(&mut accumulated, delta))
            .sum()
    }

    #[test]
    fn one_wheel_notch_is_one_step() {
        // Smoothed over a few frames: wheel down is negative, wheel up positive.
        assert_eq!(total_steps(&[-10.0, -15.0, -10.0, -5.0]), 1);
        assert_eq!(total_steps(&[10.0, 15.0, 10.0, 5.0]), -1);
    }

    #[test]
    fn a_single_notch_in_float_rounding_still_counts() {
        assert_eq!(total_steps(&[-13.333_333, -13.333_333, -13.333_332]), 1);
    }

    #[test]
    fn several_quick_notches_are_several_steps() {
        assert_eq!(total_steps(&[-20.0; 6]), 3);
        assert_eq!(total_steps(&[20.0; 4]), -2);
    }

    #[test]
    fn a_tiny_scroll_does_nothing() {
        assert_eq!(total_steps(&[-4.0, -4.0, -4.0]), 0);
    }

    #[test]
    fn a_huge_scroll_gives_many_steps() {
        assert_eq!(total_steps(&[-400.0]), 10);
    }

    #[test]
    fn stepping_moves_within_the_options() {
        assert_eq!(step_index(Some(2), 5, 1), Some(3));
        assert_eq!(step_index(Some(2), 5, -1), Some(1));
        assert_eq!(step_index(Some(1), 5, 3), Some(4));
    }

    #[test]
    fn stepping_stops_at_the_ends() {
        assert_eq!(step_index(Some(4), 5, 1), None);
        assert_eq!(step_index(Some(0), 5, -1), None);
        assert_eq!(step_index(Some(3), 5, 9), Some(4));
        assert_eq!(step_index(Some(1), 5, -9), Some(0));
    }

    #[test]
    fn no_current_option_lands_on_an_end() {
        assert_eq!(step_index(None, 5, 1), Some(0));
        assert_eq!(step_index(None, 5, -1), Some(4));
    }

    #[test]
    fn degenerate_inputs_change_nothing() {
        assert_eq!(step_index(Some(0), 0, 1), None);
        assert_eq!(step_index(None, 0, -1), None);
        assert_eq!(step_index(Some(1), 3, 0), None);
    }
}
