use iced::{Event, Length, Point, Rectangle, Size, Vector, alignment};
use iced_core::{
    Layout, Shell, Widget, mouse, overlay, renderer,
    widget::{self, Meta, tree},
};

use crate::{GlobalElement, Projector, Viewpoint};

pub struct MapLayers<'a, W, Message, Theme, Renderer> {
    base: W,
    children: Vec<GlobalElement<'a, Message, Theme, Renderer>>,
    viewpoint: Viewpoint,
}

impl<'a, W, Message, Theme, Renderer> MapLayers<'a, W, Message, Theme, Renderer> {
    pub fn new(
        base: W,
        viewpoint: Viewpoint,
        children: Vec<GlobalElement<'a, Message, Theme, Renderer>>,
    ) -> Self {
        Self {
            base,
            children,
            viewpoint,
        }
    }
}

impl<'a, W, Message, Theme, Renderer> Meta for MapLayers<'a, W, Message, Theme, Renderer> {}

impl<'a, W, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for MapLayers<'a, W, Message, Theme, Renderer>
where
    W: Widget<Message, Theme, Renderer>,
    Renderer: iced_core::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.base.size()
    }

    fn layout(
        &mut self,
        tree: &mut tree::Tree,
        renderer: &Renderer,
        limits: &iced_core::layout::Limits,
    ) {
        // 1. Layout the base map first
        // It provides the context/bounds for the projection
        self.base.layout(&mut tree.children[0], renderer, limits);

        // We use the base child's size, but we ensure the projector bounds start
        // at 0,0 because the children are placed relative to this widget's origin.
        let base_size = tree.children[0].size;
        let bounds = Rectangle::new(Point::ORIGIN, base_size);

        let projector = Projector {
            viewpoint: self.viewpoint,
            bounds,
        };

        for (i, child) in self.children.iter_mut().enumerate() {
            let child_tree = &mut tree.children[i + 1];

            // Layout children with relaxed limits (0 to the base map's size)
            child.element.layout(
                child_tree,
                renderer,
                &iced_core::layout::Limits::new(Size::ZERO, base_size),
            );

            let child_size = child_tree.size;
            let position = child.position;

            // Project geodetical position to relative screen coordinates
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

            child_tree.translation = Vector::new(x, y);
        }

        tree.size = base_size;
    }

    fn diff(&mut self, tree: &mut tree::Tree) {
        // 1. Ensure the tree has a child for the base
        if tree.children.is_empty() {
            tree.children.push(tree::Tree::new(&self.base));
        }

        // 2. Diff the base map (always index 0)
        tree.children[0].diff(&mut self.base);

        // 3. Diff existing children and append new ones
        for (i, child) in self.children.iter_mut().enumerate() {
            let idx = i + 1;
            if idx >= tree.children.len() {
                tree.children.push(tree::Tree::new(&child.element));
            }
            tree.children[idx].diff(&mut child.element);
        }

        // 4. Remove excess children if we shrank
        tree.children.truncate(self.children.len() + 1);
    }

    fn operate(
        &mut self,
        tree: &mut tree::Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.container(None, layout.bounds(), viewport);
        operation.traverse(&mut |operation| {
            let mut layouts = layout.iter_mut(&mut tree.children);
            let (base_layout, base_tree) = layouts.next().unwrap();

            self.base
                .operate(base_tree, base_layout, viewport, renderer, operation);

            for (child, (child_layout, child_tree)) in self.children.iter_mut().zip(layouts) {
                child
                    .element
                    .operate(child_tree, child_layout, viewport, renderer, operation);
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut tree::Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let mut layouts = layout.iter_mut(&mut tree.children);
        let (base_layout, base_tree) = layouts.next().unwrap();

        // Update children first (reverse order - top to bottom)
        // This allows children to capture events before the map
        for (child, (child_layout, child_tree)) in self.children.iter_mut().rev().zip(layouts.rev())
        {
            child.element.update(
                child_tree,
                event,
                child_layout,
                cursor,
                renderer,
                shell,
                viewport,
            );

            if shell.is_event_captured() {
                return;
            }
        }

        // Update base map
        self.base.update(
            base_tree,
            event,
            base_layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &tree::Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let mut layouts = layout.iter(&tree.children);
        let (base_layout, base_tree) = layouts.next().unwrap();

        // Check children interactions first (reverse order - top to bottom)
        for (child, (child_layout, child_tree)) in self.children.iter().rev().zip(layouts.rev()) {
            let interaction = child.element.mouse_interaction(
                child_tree,
                child_layout,
                cursor,
                viewport,
                renderer,
            );

            if interaction != mouse::Interaction::None {
                return interaction;
            }
        }

        // Fallback to base map interaction
        self.base
            .mouse_interaction(base_tree, base_layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &tree::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let mut layouts = layout.iter(&tree.children);
        let (base_layout, base_tree) = layouts.next().unwrap();

        // 1. Draw base map
        self.base.draw(
            base_tree,
            renderer,
            theme,
            style,
            base_layout,
            cursor,
            viewport,
        );

        // 2. Draw children on top
        renderer.with_layer(layout.bounds(), |renderer| {
            for (child, (child_layout, child_tree)) in self.children.iter().zip(layouts) {
                if child_layout.bounds().intersects(viewport) {
                    child.element.draw(
                        child_tree,
                        renderer,
                        theme,
                        style,
                        child_layout,
                        cursor,
                        viewport,
                    );
                }
            }
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut tree::Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, Renderer>> {
        let mut layouts = layout.iter_mut(&mut tree.children);
        let (base_layout, base_tree) = layouts.next().unwrap();

        let mut overlays = Vec::new();

        overlays.extend(self.base.overlay(
            base_tree,
            base_layout,
            renderer,
            viewport,
            translation,
            window,
        ));

        for (child, (child_layout, child_tree)) in self.children.iter_mut().zip(layouts) {
            overlays.extend(child.element.overlay(
                child_tree,
                child_layout,
                renderer,
                viewport,
                translation,
                window,
            ));
        }

        overlays
    }
}
