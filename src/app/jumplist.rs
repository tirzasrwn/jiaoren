//! Vim-style jump history: bounded back/forward stacks of scroll offsets, with
//! no egui dependency so the bookkeeping stays unit-testable.

/// Most positions a stack keeps; older ones fall off the front.
const JUMP_CAP: usize = 64;

/// Back/forward scroll positions for `Ctrl+O` and `Ctrl+I`.
#[derive(Default)]
pub(crate) struct JumpList {
    back: Vec<f32>,
    forward: Vec<f32>,
}

impl JumpList {
    /// Remember the pre-jump position and drop the forward trail, the way vim
    /// discards redo history after a new edit.
    pub fn record(&mut self, scroll_y: f32) {
        push_capped(&mut self.back, scroll_y);
        self.forward.clear();
    }

    /// Step back, parking `current` on the forward stack. `None` when empty.
    pub fn take_back(&mut self, current: f32) -> Option<f32> {
        let target = self.back.pop()?;
        push_capped(&mut self.forward, current);
        Some(target)
    }

    /// Step forward, parking `current` on the back stack. `None` when empty.
    pub fn take_forward(&mut self, current: f32) -> Option<f32> {
        let target = self.forward.pop()?;
        push_capped(&mut self.back, current);
        Some(target)
    }
}

/// Push `value`, dropping the oldest entry once the stack is full.
fn push_capped(stack: &mut Vec<f32>, value: f32) {
    if stack.len() == JUMP_CAP {
        stack.remove(0);
    }
    stack.push(value);
}

#[cfg(test)]
mod tests {
    use super::JumpList;

    #[test]
    fn empty_list_has_nothing_to_take() {
        let mut j = JumpList::default();
        assert_eq!(j.take_back(10.0), None);
        assert_eq!(j.take_forward(10.0), None);
    }

    #[test]
    fn back_then_forward_round_trip() {
        let mut j = JumpList::default();
        j.record(10.0);
        j.record(20.0);
        assert_eq!(j.take_back(30.0), Some(20.0));
        assert_eq!(j.take_back(20.0), Some(10.0));
        assert_eq!(j.take_back(10.0), None);
        assert_eq!(j.take_forward(10.0), Some(20.0));
        assert_eq!(j.take_forward(20.0), Some(30.0));
        assert_eq!(j.take_forward(30.0), None);
    }

    #[test]
    fn new_jump_clears_forward() {
        let mut j = JumpList::default();
        j.record(1.0);
        assert_eq!(j.take_back(2.0), Some(1.0));
        j.record(3.0);
        assert_eq!(j.take_forward(4.0), None);
        assert_eq!(j.take_back(4.0), Some(3.0));
    }

    #[test]
    fn back_stack_keeps_the_last_64() {
        let mut j = JumpList::default();
        for i in 0..70 {
            j.record(i as f32);
        }
        let mut seen = Vec::new();
        while let Some(y) = j.take_back(999.0) {
            seen.push(y);
        }
        assert_eq!(seen.len(), 64);
        assert_eq!(seen[0], 69.0);
        assert_eq!(seen[63], 6.0);
    }
}
