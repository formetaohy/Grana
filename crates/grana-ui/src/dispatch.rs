use glam::Vec2;

use crate::edit::{Edit, Outcome};
use crate::element::{Axis, Content, Interaction, Phase, Pointer};
use crate::hit;
use crate::input::{Button, Key as InputKey};
use crate::node::NodeId;
use crate::ui::Ui;

impl<A: Clone> Ui<A> {
    pub(crate) fn dispatch(&mut self) {
        let pointer = self.pointer;
        self.hover = hit::resolve(&self.hits, pointer).map(|hit| hit.node);
        if let Some(node) = self.hover {
            let content_cursor = crate::widget::cursor(&self.tree.nodes[node].content);
            if let Some(cursor) = self.tree.nodes[node].cursor.or(content_cursor) {
                self.cursor = cursor;
            }
            if matches!(
                self.tree.nodes[node].content,
                Content::Segment(_) | Content::Tabs(_)
            ) {
                let content = &self.tree.nodes[node].content;
                let rect = self.tree.nodes[node].rect;
                let index = crate::widget::segment::cell_at(content, rect, pointer, &mut self.text);
                self.tree.nodes[node].runtime.thumb =
                    index.map_or(0, |index| index.min(u8::MAX as usize) as u8);
            }
        }
        let primary = self.input.button(Button::Primary);
        let editing = self.edit;
        if primary.pressed
            && let Some(node) = editing
            && self.tree.nodes[node].rect.contains(pointer)
        {
            self.edit_press(node);
        } else if primary.pressed {
            self.press = self.hover;
            self.press_at = pointer;
            self.drag = None;
            self.focus_visible = false;
            self.dismiss_popups();
        } else if primary.down && self.edit_drag {
            self.edit_drag_to(pointer);
        }
        if primary.down
            && let Some(press) = self.press
            && self.drag.is_none()
            && (pointer - self.press_at).length() > crate::ui::CLICK_SLOP
        {
            self.drag = Some(press);
        }
        self.drag_values();
        if primary.down
            && let Some(press) = self.press
        {
            self.pointer_events(
                press,
                if primary.pressed {
                    Phase::Down
                } else {
                    Phase::Move
                },
            );
        }
        if primary.released {
            if let Some(press) = self.press {
                let dragged = self.drag == Some(press);
                let toggle = matches!(
                    self.tree.nodes[press].interaction,
                    Interaction::Toggle { .. }
                );
                let switch = toggle
                    && match &self.tree.nodes[press].content {
                        Content::Toggle(content) => {
                            let rect = self.tree.nodes[press].rect;
                            crate::widget::toggle::switch_rect(content, rect, &self.theme.metrics)
                                .contains(self.press_at)
                        }
                        _ => false,
                    };
                let segment = dragged
                    && matches!(
                        self.tree.nodes[press].content,
                        Content::Segment(_) | Content::Tabs(_)
                    );
                let clicked = self.drag.is_none()
                    && self.hover.is_none_or(|hover| hover == press)
                    && !self.edit_drag;
                if dragged && switch {
                    self.commit_toggle(press);
                } else if segment {
                    self.commit_segment(press);
                } else if clicked || (dragged && toggle) {
                    self.activate(press);
                }
                self.pointer_events(press, Phase::Up);
            }
            self.press = None;
            self.drag = None;
            self.edit_drag = false;
        }
        self.wheel();
        if self.edit.is_some() {
            self.field_keys();
        } else {
            self.keys();
        }
    }

    fn wheel(&mut self) {
        let wheel = self.input.wheel();
        if wheel == Vec2::ZERO {
            return;
        }
        let pointer = self.pointer;
        let Some(node) = hit::resolve_scroll(&self.hits, pointer).map(|hit| hit.node) else {
            return;
        };
        if let Some(top) = hit::top(&self.hits, pointer)
            && top.layer > self.tree.nodes[node].layer.index()
        {
            return;
        }
        let amount = match self.tree.nodes[node].scroll_axis() {
            Axis::Column => -wheel.y,
            Axis::Row => -wheel.x - wheel.y,
        };
        if amount == 0.0 {
            return;
        }
        if let Some(scroll) = self.tree.nodes[node].scroll.as_mut() {
            scroll.scroll_by(amount * 42.0);
        }
        self.tree.nodes[node].paint_dirty = true;
        self.tree.layout_dirty = true;
        self.animating = true;
    }

    fn drag_values(&mut self) {
        let Some(id) = self.drag.or(self.press) else {
            return;
        };
        if !self.tree.nodes[id].enabled {
            return;
        }
        let dragging = self.drag == Some(id);
        let pressed = self.press == Some(id);
        let pointer = self.pointer;
        let delta = self.input.delta().x;
        let shift = self.input.modifiers().shift;
        let text = &mut self.text;
        let theme = &self.theme;
        let node = &mut self.tree.nodes[id];
        let crate::node::Node {
            content,
            interaction,
            runtime,
            rect,
            ..
        } = node;
        let rect = *rect;
        let mut action = None;
        match interaction {
            Interaction::Slider { action: callback } if pressed => {
                if let Content::Slider(slider) = content {
                    let value = crate::widget::slider::value_at(slider, rect, pointer, text, theme);
                    runtime.drag_value = crate::widget::slider::normalized_value(slider, value);
                    if (value - slider.value).abs() > 1.0e-6 {
                        action = Some(callback(value));
                    }
                }
            }
            Interaction::RangeSlider { action: callback } if pressed => {
                if let Content::RangeSlider(range) = content {
                    let (low, high) = crate::widget::slider::range_at(range, rect, pointer);
                    let span = (*range.range.end() - *range.range.start()).max(1.0e-6);
                    runtime.drag_value = (low - *range.range.start()) / span;
                    runtime.drag_extent = (high - *range.range.start()) / span;
                    if (low - range.start).abs() > 1.0e-6 || (high - range.end).abs() > 1.0e-6 {
                        action = Some(callback(low, high));
                    }
                }
            }
            Interaction::Segment { action: callback } if dragging => {
                let index = crate::widget::segment::cell_at(content, rect, pointer, text);
                if let Some(index) = index
                    && index != crate::widget::segment::selected(content)
                {
                    action = Some(callback(index));
                }
            }
            Interaction::Toggle { .. } if dragging => {
                if let Content::Toggle(toggle) = content {
                    let switch = crate::widget::toggle::switch_rect(toggle, rect, &theme.metrics);
                    runtime.drag_value = if switch.contains(self.press_at) {
                        crate::widget::toggle::drag_value(switch, pointer)
                    } else {
                        crate::widget::toggle::normalized(toggle)
                    };
                }
            }
            Interaction::Color { action: callback } if pressed => {
                if let Content::ColorPicker(picker) = content {
                    let color = picker.color;
                    if let Some((picked, hue)) = crate::widget::color::pick(
                        rect,
                        color,
                        runtime.value.value,
                        runtime.seeded,
                        pointer,
                    ) {
                        runtime.value.snap(hue);
                        runtime.seeded = true;
                        if (picked[0] - color[0]).abs()
                            + (picked[1] - color[1]).abs()
                            + (picked[2] - color[2]).abs()
                            > 1.0e-6
                        {
                            action = Some(callback(picked));
                        }
                    }
                }
            }
            Interaction::DragValue {
                speed,
                action: callback,
            } if dragging => {
                if let Content::DragValue(drag) = content
                    && delta != 0.0
                {
                    let scale = if shift { 0.1 } else { 1.0 };
                    action = Some(callback(drag.value + delta * *speed * scale));
                }
            }
            _ => {}
        }
        if let Some(action) = action {
            self.actions.push(action);
        }
    }
    fn pointer_events(&mut self, node: NodeId, phase: Phase) {
        let action = {
            let node = &self.tree.nodes[node];
            if let Interaction::Pointer { handler } = &node.interaction {
                handler(Pointer {
                    position: self.pointer,
                    delta: self.input.delta(),
                    phase,
                    button: Button::Primary,
                    modifiers: self.input.modifiers(),
                })
            } else {
                None
            }
        };
        if let Some(action) = action {
            self.actions.push(action);
        }
    }

    fn dismiss_popups(&mut self) {
        let pointer = self.pointer;
        let pressed = self.press;
        let open = self
            .popup_anchors
            .iter()
            .filter(|(_, id)| self.tree.nodes[**id].runtime.open)
            .map(|(anchor, id)| (*anchor, *id))
            .collect::<Vec<_>>();
        for (anchor, popup) in open {
            if pressed == self.tree.node_at(anchor) {
                continue;
            }
            let inside = self.tree.nodes[popup].rect.contains(pointer)
                || pressed.is_some_and(|node| is_inside(&self.tree, popup, node));
            if !inside {
                self.set_popup(popup, false);
            }
        }
    }

    fn activate(&mut self, node: NodeId) {
        let key = self.tree.nodes[node].key;
        if let Some(&popup) = self.popup_anchors.get(&key)
            && popup != node
        {
            let open = !self.tree.nodes[popup].runtime.open;
            self.set_popup(popup, open);
            self.focus = Some(node);
            return;
        }
        if matches!(self.tree.nodes[node].interaction, Interaction::Field { .. }) {
            if self.edit != Some(node) {
                self.begin_edit(node, true);
            }
            self.focus = Some(node);
            return;
        }
        if matches!(
            self.tree.nodes[node].content,
            Content::Segment(_) | Content::Tabs(_)
        ) {
            let content = &self.tree.nodes[node].content;
            let rect = self.tree.nodes[node].rect;
            let pointer = self.pointer;
            let index = crate::widget::segment::cell_at(content, rect, pointer, &mut self.text);
            if let Some(index) = index
                && let Interaction::Segment { action } = &self.tree.nodes[node].interaction
            {
                let action = action(index);
                self.close_ancestor_popup(node);
                self.actions.push(action);
            }
            self.focus = Some(node);
            return;
        }
        let action = match &self.tree.nodes[node].interaction {
            Interaction::Press(action) => Some(action.clone()),
            Interaction::Toggle { action } => {
                let value = match &self.tree.nodes[node].content {
                    Content::Toggle(toggle) => toggle.value,
                    Content::Checkbox(checkbox) => checkbox.value,
                    _ => false,
                };
                Some(action(!value))
            }
            _ => None,
        };
        if let Some(action) = action {
            self.close_ancestor_popup(node);
            self.actions.push(action);
        }
        self.focus = Some(node);
    }

    fn commit_toggle(&mut self, node: NodeId) {
        if !self.tree.nodes[node].enabled {
            return;
        }
        let current = match &self.tree.nodes[node].content {
            Content::Toggle(toggle) => toggle.value,
            _ => return,
        };
        let value = self.tree.nodes[node].runtime.drag_value >= 0.5;
        self.tree.nodes[node].runtime.commit = Some(if value { 1.0 } else { 0.0 });
        let action = if value != current {
            if let Interaction::Toggle { action } = &self.tree.nodes[node].interaction {
                Some(action(value))
            } else {
                None
            }
        } else {
            None
        };
        if let Some(action) = action {
            self.close_ancestor_popup(node);
            self.actions.push(action);
        }
        self.focus = Some(node);
    }

    fn commit_segment(&mut self, node: NodeId) {
        let rect = self.tree.nodes[node].rect;
        let index = {
            let content = &self.tree.nodes[node].content;
            crate::widget::segment::cell_at(content, rect, self.pointer, &mut self.text)
        };
        if let Some(index) = index {
            self.tree.nodes[node].runtime.commit = Some(index as f32);
        }
    }

    fn close_ancestor_popup(&mut self, node: NodeId) {
        let mut current = Some(node);
        while let Some(id) = current {
            if self.tree.nodes[id].popup_spec.is_some() {
                self.set_popup(id, false);
                return;
            }
            current = self.tree.nodes[id].parent;
        }
    }

    fn set_popup(&mut self, node: NodeId, open: bool) {
        self.tree.nodes[node].runtime.open = open;
        self.tree.nodes[node].paint_dirty = true;
        self.tree.mark_dirty();
    }

    pub(crate) fn begin_edit(&mut self, node: NodeId, at_pointer: bool) {
        let (value, multiline) = match &self.tree.nodes[node].content {
            Content::Field(field) => (field.value.clone(), field.multiline),
            _ => return,
        };
        let key = self.tree.nodes[node].key;
        let mut edit = if multiline {
            Edit::multiline(key.id(), &value, self.time)
        } else {
            Edit::new(key.id(), &value, self.time)
        };
        if at_pointer {
            let Content::Field(field) = &self.tree.nodes[node].content else {
                return;
            };
            let rect = self.tree.nodes[node].rect;
            let caret = crate::widget::field::caret_at(field, rect, self.pointer, &mut self.text);
            edit.place(caret, false, self.time);
            edit.click(self.pointer, self.time);
        }
        self.tree.nodes[node].runtime.edit = Some(edit);
        self.edit = Some(node);
        self.focus = Some(node);
        self.focus_visible = false;
        self.input.clear_preedit();
    }

    fn edit_press(&mut self, node: NodeId) {
        let Content::Field(field) = &self.tree.nodes[node].content else {
            return;
        };
        let rect = self.tree.nodes[node].rect;
        let caret = crate::widget::field::caret_at(field, rect, self.pointer, &mut self.text);
        let extend = self.input.modifiers().shift;
        let time = self.time;
        let pointer = self.pointer;
        let Some(edit) = self.tree.nodes[node].runtime.edit.as_mut() else {
            return;
        };
        edit.place(caret, extend, time);
        let clicks = edit.click(pointer, time);
        match clicks {
            2 => {
                let (start, end) = edit.word(caret);
                edit.select((start, end), time);
            }
            3 => edit.select_all(time),
            _ => {}
        }
        self.edit_drag = clicks == 1;
        self.focus = Some(node);
        self.focus_visible = false;
    }

    fn edit_drag_to(&mut self, pointer: Vec2) {
        let Some(node) = self.edit else {
            return;
        };
        let Content::Field(field) = &self.tree.nodes[node].content else {
            return;
        };
        let rect = self.tree.nodes[node].rect;
        let caret = crate::widget::field::caret_at(field, rect, pointer, &mut self.text);
        let time = self.time;
        if let Some(edit) = self.tree.nodes[node].runtime.edit.as_mut() {
            edit.place(caret, true, time);
        }
    }

    fn field_keys(&mut self) {
        let Some(node) = self.edit else {
            return;
        };
        let commit = {
            let Some(edit) = self.tree.nodes[node].runtime.edit.as_mut() else {
                self.edit = None;
                return;
            };
            match edit.poll(&self.input, self.clipboard.as_mut(), self.time) {
                Some(Outcome::Commit) => Some(true),
                Some(Outcome::Cancel) => Some(false),
                None => {
                    let primary = self.input.button(Button::Primary);
                    if primary.pressed && !self.tree.nodes[node].rect.contains(self.pointer) {
                        Some(true)
                    } else {
                        None
                    }
                }
            }
        };
        if let Some(commit) = commit {
            let text = self.tree.nodes[node]
                .runtime
                .edit
                .take()
                .map_or_else(String::new, |edit| edit.text().to_string());
            self.edit = None;
            let action = match &self.tree.nodes[node].interaction {
                Interaction::Field { commit: apply, .. } if commit => Some(apply(&text)),
                Interaction::Field {
                    cancel: Some(cancel),
                    ..
                } => Some((cancel)()),
                _ => None,
            };
            if let Some(action) = action {
                self.actions.push(action);
            }
        }
    }

    fn keys(&mut self) {
        let modifiers = self.input.modifiers();
        let keys = self
            .input
            .keys()
            .iter()
            .filter(|event| event.pressed && !event.repeat)
            .filter_map(|event| event.key)
            .collect::<Vec<_>>();
        if keys.contains(&InputKey::Tab) {
            if self.advance_focus(!modifiers.shift) {
                self.focus_visible = true;
            }
            return;
        }
        if keys.contains(&InputKey::Escape) {
            for popup in self.popup_anchors.values().copied().collect::<Vec<_>>() {
                if self.tree.nodes[popup].runtime.open {
                    self.set_popup(popup, false);
                }
            }
            self.focus = None;
            self.focus_visible = false;
            return;
        }
        let Some(node) = self.focus else {
            return;
        };
        for key in keys {
            if self.adjust(node, key) {
                self.focus_visible = true;
            }
        }
        if self.focus_visible
            && self.input.keys().iter().any(|event| {
                event.pressed
                    && !event.repeat
                    && matches!(event.key, Some(InputKey::Enter | InputKey::Space))
            })
        {
            self.activate(node);
        }
    }

    fn adjust(&mut self, node: NodeId, key: InputKey) -> bool {
        let direction = match key {
            InputKey::Left | InputKey::Down => -1.0,
            InputKey::Right | InputKey::Up => 1.0,
            InputKey::Home | InputKey::End | InputKey::PageUp | InputKey::PageDown => 0.0,
            _ => return false,
        };
        match &self.tree.nodes[node].content {
            Content::Slider(slider) => {
                let value = match key {
                    InputKey::Home => *slider.range.start(),
                    InputKey::End => *slider.range.end(),
                    InputKey::PageUp => crate::widget::slider::step(slider, 1.0, true),
                    InputKey::PageDown => crate::widget::slider::step(slider, -1.0, true),
                    _ => crate::widget::slider::step(slider, direction, false),
                };
                if (value - slider.value).abs() <= 1.0e-6 {
                    return true;
                }
                if let Interaction::Slider { action } = &self.tree.nodes[node].interaction {
                    let action = action(value);
                    self.actions.push(action);
                }
                true
            }
            Content::RangeSlider(range) => {
                let (start, end) = match key {
                    InputKey::Home => (*range.range.start(), range.end),
                    InputKey::End => (range.start, *range.range.end()),
                    InputKey::PageUp => crate::widget::slider::range_step(range, 1.0, true),
                    InputKey::PageDown => crate::widget::slider::range_step(range, -1.0, true),
                    _ => crate::widget::slider::range_step(range, direction, false),
                };
                if (start - range.start).abs() <= 1.0e-6 && (end - range.end).abs() <= 1.0e-6 {
                    return true;
                }
                if let Interaction::RangeSlider { action } = &self.tree.nodes[node].interaction {
                    let action = action(start, end);
                    self.actions.push(action);
                }
                true
            }
            Content::Segment(_) | Content::Tabs(_) => {
                let content = &self.tree.nodes[node].content;
                let index = match key {
                    InputKey::Home => 0,
                    InputKey::End => crate::widget::segment::options_len(content).saturating_sub(1),
                    _ => crate::widget::segment::step_index(content, direction as isize),
                };
                if index == crate::widget::segment::selected(content) {
                    return true;
                }
                if let Interaction::Segment { action } = &self.tree.nodes[node].interaction {
                    let action = action(index);
                    self.actions.push(action);
                }
                true
            }
            Content::DragValue(drag) => {
                let value = match key {
                    InputKey::PageUp => crate::widget::slider::drag_step(drag, 1.0, true),
                    InputKey::PageDown => crate::widget::slider::drag_step(drag, -1.0, true),
                    InputKey::Home | InputKey::End => return true,
                    _ => crate::widget::slider::drag_step(drag, direction, false),
                };
                if let Interaction::DragValue { action, .. } = &self.tree.nodes[node].interaction {
                    let action = action(value);
                    self.actions.push(action);
                }
                true
            }
            _ => false,
        }
    }
}

fn is_inside<A>(tree: &crate::tree::Tree<A>, root: NodeId, node: NodeId) -> bool {
    let mut current = Some(node);
    while let Some(id) = current {
        if id == root {
            return true;
        }
        current = tree.nodes[id].parent;
    }
    false
}
