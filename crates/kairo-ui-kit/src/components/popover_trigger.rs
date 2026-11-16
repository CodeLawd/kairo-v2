//! Popover triggers that assistive technology can press (prompt 106).
//!
//! A Kit `Popover` opens on a mouse press on its trigger. A Kit `Button`
//! without a click handler does not tell VoiceOver or Narrator it can be
//! pressed, so the trigger read as a button that does nothing. With a click
//! handler the node offers `Click`, and GPUI answers an assistive press it
//! has no listener for with a mouse press and release at the node's centre,
//! which the popover handles as it handles the pointer.

use gpui_kit::component::button::Button;

/// Marks a Kit `Button` as a popover trigger.
pub trait PopoverTriggerExt {
    /// Lets assistive technology press the trigger. The popover still does
    /// the opening and closing.
    fn popover_trigger(self) -> Self;
}

impl PopoverTriggerExt for Button {
    fn popover_trigger(self) -> Self {
        self.on_click(|_, _, _| {})
    }
}

/// Sets a Kit `Button` as a popover's trigger, pressable from assistive
/// technology (see [`PopoverTriggerExt`]).
pub trait PopoverButtonTriggerExt {
    /// `trigger(button.popover_trigger())`.
    fn button_trigger(self, button: Button) -> Self;
}

impl PopoverButtonTriggerExt for gpui_kit::component::popover::Popover {
    fn button_trigger(self, button: Button) -> Self {
        self.trigger(button.popover_trigger())
    }
}
