use crate::Mercator;
use iced::Widget as _;
use iced::{Element, alignment};
use iced_core::Widget;

/// Like a regular [`Element`] but tied to a specific [`Geodetic`] coordinate
pub struct GlobalElement<'a, Message, Theme, Renderer> {
    pub element: Element<'a, Message, Theme, Renderer>,
    pub position: Mercator,
    pub horizontal_alignment: alignment::Horizontal,
    pub vertical_alignment: alignment::Vertical,
}

impl<'a, Message, Theme, Renderer> GlobalElement<'a, Message, Theme, Renderer> {
    pub fn new(element: impl Widget<Message, Theme, Renderer> + 'a, position: Mercator) -> Self {
        Self {
            element: element.boxed(),
            position,
            horizontal_alignment: alignment::Horizontal::Center,
            vertical_alignment: alignment::Vertical::Center,
        }
    }

    pub fn align(
        mut self,
        horizontal: alignment::Horizontal,
        vertical: alignment::Vertical,
    ) -> Self {
        self.horizontal_alignment = horizontal;
        self.vertical_alignment = vertical;
        self
    }
}
