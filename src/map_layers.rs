use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, alignment};
use iced_core::{self, Clipboard, Layout, Shell, Widget, mouse, overlay, renderer, widget::tree};

use crate::{GlobalElement, Projector, Viewpoint};

pub struct MapLayers<'a, Message, Theme, Renderer> {
    base: Element<'a, Message, Theme, Renderer>,
    children: Vec<GlobalElement<'a, Message, Theme, Renderer>>,
    viewpoint: Viewpoint,
}

impl<'a, Message, Theme, Renderer> MapLayers<'a, Message, Theme, Renderer>
where
    Renderer: iced_core::Renderer,
{
    pub fn new(
        base: impl Into<Element<'a, Message, Theme, Renderer>>,
        viewpoint: Viewpoint,
        children: Vec<GlobalElement<'a, Message, Theme, Renderer>>,
    ) -> Self {
        Self {
            base: base.into(),
            children,
            viewpoint,
        }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for MapLayers<'a, Message, Theme, Renderer>
where
    Renderer: iced_core::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut tree::Tree,
        renderer: &Renderer,
        limits: &iced_core::layout::Limits,
    ) -> iced_core::layout::Node {
        // 1. Layout the base map first
        // It provides the context/bounds for the projection
        let base_node = self
            .base
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);

        // We use the base node's size, but we ensure the projector bounds start at 0,0
        // because the children nodes will be placed relative to this widget's origin.
        let bounds = Rectangle::new(Point::ORIGIN, base_node.bounds().size());

        let projector = Projector {
            viewpoint: self.viewpoint,
            cursor: None,
            bounds,
        };

        let mut nodes = Vec::with_capacity(1 + self.children.len());
        nodes.push(base_node);

        for (i, child) in self.children.iter_mut().enumerate() {
            // Layout children with relaxed limits (0 to max)
            let child_node = child.element.as_widget_mut().layout(
                &mut tree.children[i + 1],
                renderer,
                &iced_core::layout::Limits::new(Size::ZERO, limits.max()),
            );

            let child_size = child_node.size();
            let position = child.position;

            // Project geographical position to relative screen coordinates
            let screen_pos = projector.mercator_into_screen_space(position);

            let x = match child.horizontal_alignment {
                alignment::Horizontal::Left => screen_pos.x,
                alignment::Horizontal::Center => screen_pos.x - child_size.width / 2.0,
                alignment::Horizontal::Right => screen_pos.x - child_size.width,
            };

            let y = match child.vertical_alignment {
                alignment::Vertical::Top => screen_pos.y,
                alignment::Vertical::Center => screen_pos.y - child_size.height / 2.0,
                alignment::Vertical::Bottom => screen_pos.y - child_size.height,
            };

            nodes.push(child_node.move_to(Point::new(x, y)));
        }

        iced_core::layout::Node::with_children(bounds.size(), nodes)
    }

    fn children(&self) -> Vec<tree::Tree> {
        let mut children = vec![tree::Tree::new(&self.base)];
        children.extend(self.children.iter().map(|c| tree::Tree::new(&c.element)));
        children
    }

    fn diff(&self, tree: &mut tree::Tree) {
        // 1. Diff the base map (always index 0)
        tree.children[0].diff(&self.base);

        // 2. Diff existing children and append new ones
        for (i, child) in self.children.iter().enumerate() {
            let idx = i + 1;
            if idx < tree.children.len() {
                tree.children[idx].diff(&child.element);
            } else {
                tree.children.push(tree::Tree::new(&child.element));
            }
        }

        // 3. Remove excess children if we shrank
        tree.children.truncate(self.children.len() + 1);
    }

    fn update(
        &mut self,
        tree: &mut tree::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let mut children_layout = layout.children();
        let base_layout = children_layout.next().unwrap();
        let layouts: Vec<_> = children_layout.collect();

        // Update children first (reverse order - top to bottom)
        // This allows children to capture events before the map
        for (i, child) in self.children.iter_mut().enumerate().rev() {
            let child_tree = &mut tree.children[i + 1];
            let child_layout = layouts[i];

            child.element.as_widget_mut().update(
                child_tree,
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );

            if shell.is_event_captured() {
                return;
            }
        }

        // Update base map
        self.base.as_widget_mut().update(
            &mut tree.children[0],
            event,
            base_layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &tree::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let mut children_layout = layout.children();
        let base_layout = children_layout.next().unwrap();
        let layouts: Vec<_> = children_layout.collect();

        // Check children interactions first
        for (i, child) in self.children.iter().enumerate().rev() {
            let child_tree = &tree.children[i + 1];
            let child_layout = layouts[i];

            let interaction = child.element.as_widget().mouse_interaction(
                child_tree,
                child_layout,
                cursor,
                viewport,
                renderer,
            );

            if interaction != mouse::Interaction::Idle {
                return interaction;
            }
        }

        // Fallback to base map interaction
        self.base.as_widget().mouse_interaction(
            &tree.children[0],
            base_layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &tree::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let mut children_layout = layout.children();
        let base_layout = children_layout.next().unwrap();

        // 1. Draw base map
        self.base.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            base_layout,
            cursor,
            viewport,
        );

        // 2. Draw children on top
        renderer.with_layer(layout.bounds(), |renderer| {
            for (i, child) in self.children.iter().enumerate() {
                let child_tree = &tree.children[i + 1];
                let child_layout = children_layout.next().unwrap();

                child.element.as_widget().draw(
                    child_tree,
                    renderer,
                    theme,
                    style,
                    child_layout,
                    cursor,
                    viewport,
                );
            }
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut tree::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let mut children_layout = layout.children();
        let base_layout = children_layout.next().unwrap();

        let mut overlays = Vec::new();

        // Split tree to access base independently from children
        let (base_tree, children_trees) = tree.children.split_first_mut().unwrap();

        if let Some(overlay) = self.base.as_widget_mut().overlay(
            base_tree,
            base_layout,
            renderer,
            viewport,
            translation,
        ) {
            overlays.push(overlay);
        }

        for (child, child_tree) in self.children.iter_mut().zip(children_trees.iter_mut()) {
            let child_layout = children_layout.next().unwrap();
            if let Some(overlay) = child.element.as_widget_mut().overlay(
                child_tree,
                child_layout,
                renderer,
                viewport,
                translation,
            ) {
                overlays.push(overlay);
            }
        }

        (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).overlay())
    }
}

impl<'a, Message, Theme, Renderer> From<MapLayers<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced_core::Renderer + 'a,
{
    fn from(layers: MapLayers<'a, Message, Theme, Renderer>) -> Self {
        Element::new(layers)
    }
}
