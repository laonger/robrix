use makepad_widgets::*;
use std::collections::HashMap;

const MIN_DESKTOP_WIDTH: f64 = 860.;

live_design! {
    import makepad_widgets::base::*;
    import makepad_widgets::theme_desktop_dark::*;

    AdaptiveView = {{AdaptiveView}} {
        width: Fill, height: Fill
        
        Shared = <View> {}
    
        Mobile = <View> {}
        Tablet = <View> {}
        Desktop = <View> {}
    }
}

#[derive(Live, LiveRegisterWidget, WidgetRef)]
pub struct AdaptiveView {
    #[rust]
    area: Area,

    /// This widget's walk, it should always match the walk of the active widget.
    #[walk]
    walk: Walk,

    /// Wether to retain the widget variant state when it goes unused.
    /// While it avoids creating new widgets and keeps their state, be mindful of the memory usage and potential memory leaks.
    #[live]
    retain_unused_variants: bool,

    /// A map of previously active widgets that are not currently being displayed.
    /// Only used when `retain_unused_variants` is true.
    #[rust]
    previously_active_widgets: HashMap<LiveId, WidgetVariant>,

    /// A map of templates that are used to create the active widget.
    #[rust] 
    templates: ComponentMap<LiveId, LivePtr>,
    
    /// The active widget that is currently being displayed.
    #[rust] 
    active_widget: Option<WidgetVariant>,

    /// The current variant selector that determines which template to use.
    #[rust]
    variant_selector: Option<Box<VariantSelector>>,

    #[rust]
    screen_width: f64,
}

pub struct WidgetVariant {
    pub template_id: LiveId,
    pub widget_ref: WidgetRef,
}

impl WidgetNode for AdaptiveView {
    fn walk(&mut self, cx: &mut Cx) -> Walk {
        if let Some(active_widget) = self.active_widget.as_ref() {
            active_widget.widget_ref.walk(cx)
        } else {
            self.walk
        }
    }
    fn area(&self)->Area{
        self.area
    }
    
    fn redraw(&mut self, cx: &mut Cx) {
        self.area.redraw(cx);
    }

    fn find_widgets(&self, path: &[LiveId], cached: WidgetCache, results: &mut WidgetSet) {
        if let Some(active_widget) = self.active_widget.as_ref() {
            // Currently we cannot rely on querying nested elements (e.g. `self.ui.button(id!(my_button))`) within an AdaptiveView, 
            // from a non-AdaptiveView parent. This is becuase higher up in the UI tree other widgets have cached the search result.
            // Makepad should support a way to prevent caching for scenarios like this. 
            // TODO(Julian): We'll add a mechanism in Makepad to clear the cache upawards on template change (e.g. InvalidateCache action).
            active_widget.widget_ref.find_widgets(path, cached, results);
        }
    }
    
    fn uid_to_widget(&self, uid:WidgetUid) -> WidgetRef {
        if let Some(active_widget) = self.active_widget.as_ref() {
            active_widget.widget_ref.uid_to_widget(uid)
        }
        else {
            WidgetRef::empty()
        }
    }
}

impl LiveHook for AdaptiveView {
    fn before_apply(&mut self, _cx: &mut Cx, apply: &mut Apply, _index: usize, _nodes: &[LiveNode]) {
        if let ApplyFrom::UpdateFromDoc {..} = apply.from {
            self.templates.clear();
        }
    }

    fn after_apply_from_doc(&mut self, cx:&mut Cx) {
        self.set_default_variant_selector(cx);
    }
    
    // hook the apply flow to collect our templates and apply to instanced childnodes
    fn apply_value_instance(&mut self, cx: &mut Cx, apply: &mut Apply, index: usize, nodes: &[LiveNode]) -> usize {
        if nodes[index].is_instance_prop() {
            if let Some(live_ptr) = apply.from.to_live_ptr(cx, index){
                let id = nodes[index].id;
                self.templates.insert(id, live_ptr);

                if let Some(widget_variant) = self.active_widget.as_mut() {
                    if widget_variant.template_id == id {
                        widget_variant.widget_ref.apply(cx, apply, index, nodes);
                    }
                }
            }
        }
        else {
            cx.apply_error_no_matching_field(live_error_origin!(), index, nodes);
        }
        nodes.skip_node(index)
    }
}

impl Widget for AdaptiveView {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.widget_match_event(cx, event, scope);
        if let Some(active_widget) = self.active_widget.as_mut() {
            active_widget.widget_ref.handle_event(cx, event, scope);
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        if self.active_widget.is_some() {
            let mut context = cx.get_global::<DisplayContext>().clone(); // TODO(Julian) avoid cloning
            context.parent_size = cx.peek_walk_turtle(walk).size;
    
            // Apply the resize which may modify self.active_widget
            self.apply_after_resize(cx, &context);
    
            // Re-borrow to make the borrow checker happy
            if let Some(active_widget) = self.active_widget.as_mut() {
                active_widget.widget_ref.draw_walk(cx, scope, walk)?;
            }
        }
    
        DrawStep::done()
    }
}

impl WidgetMatchEvent for AdaptiveView {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, _scope: &mut Scope) {
        for action in actions {
            // Handle window geom change events to update the screen width, this is triggered at startup and on window resize
            if let WindowAction::WindowGeomChange(ce) = action.as_widget_action().cast() {
                // TODO(Julian): Fix excessive cloning
                let redraw_id = cx.redraw_id;
                if self.screen_width != ce.new_geom.inner_size.x {
                    self.screen_width = ce.new_geom.inner_size.x;
                    // Cache a query context for the current `WindowGeomChange`
                    if cx.has_global::<DisplayContext>() {
                        let current_context = cx.get_global::<DisplayContext>();
                        // TODO(Julian): Optimize this by skipping the update on the same event id for different instances
                        // We should add an accesor in Makepad for cx.event_id  
                        // if current_context.updated_on_event_id == event_id { return }
                        
                        current_context.updated_on_event_id = redraw_id;
                        current_context.screen_width = self.screen_width;
                        current_context.parent_size = DVec2::default();

                        let cloned_context = current_context.clone();
                        self.apply_after_resize(cx, &cloned_context);
                    } else {
                        let display_context = DisplayContext {
                            updated_on_event_id: cx.redraw_id,
                            screen_width: self.screen_width,
                            parent_size: DVec2::default(),
                        };

                        self.apply_after_resize(cx, &display_context);
                        cx.set_global(display_context);
                    }

                    cx.redraw_all();
                }
            }
        }
    }
}

impl AdaptiveView {
    /// Apply the variant selector to determine which template to use.
    fn apply_after_resize(&mut self, cx: &mut Cx, display_context: &DisplayContext) {
        let Some(variant_selector) = self.variant_selector.as_mut() else {return};

        let template_id = variant_selector(display_context);

        // If the selector resulted in a widget that is already active, do nothing
        if let Some(active_widget) = self.active_widget.as_mut() {
            if active_widget.template_id == template_id {
                return;
            }
        }

        // If the selector resulted in a widget that was previously active, restore it
        if self.retain_unused_variants && self.previously_active_widgets.contains_key(&template_id) {
            let widget_variant = self.previously_active_widgets.remove(&template_id).unwrap();

            self.walk = widget_variant.widget_ref.walk(cx);
            self.active_widget = Some(widget_variant);
            return;
        }

        // Create a new widget from the template
        let template = self.templates.get(&template_id).unwrap();
        let widget_ref = WidgetRef::new_from_ptr(cx, Some(*template));
        
        // Update this widget's walk to match the walk of the active widget,
        // this ensures that the new widget is not affected by `Fill` or `Fit` constraints from this parent.
        self.walk = widget_ref.walk(cx);

        if let Some(active_widget) = self.active_widget.take() {
            if self.retain_unused_variants {
                self.previously_active_widgets.insert(active_widget.template_id, active_widget);
            }
        }

        self.active_widget = Some(WidgetVariant { template_id, widget_ref });
    }

    /// Set a variant selector for this widget. 
    /// The selector is a closure that takes a `DisplayContext` and returns a `LiveId`, corresponding to the template to use.
    pub fn set_variant_selector(&mut self, cx: &mut Cx, selector: impl FnMut(&DisplayContext) -> LiveId + 'static) {
        self.variant_selector = Some(Box::new(selector));
        
        // If we have a global display context, apply the variant selector
        // This should be always done after updating selectors. This is useful for AdaptiveViews 
        // spawned after the initial resize event (e.g. PortalList items)
        // In Robrix we know there are parent AdaptiveViews that have already set the global display context,
        // but we'll have to make sure that's the case in Makepad when porting this Widget.
        if cx.has_global::<DisplayContext>() {
            let display_context = cx.get_global::<DisplayContext>().clone();
            self.apply_after_resize(cx, &display_context);
        }
    }

    pub fn set_default_variant_selector(&mut self, cx: &mut Cx) {
        // TODO(Julian): more comprehensive default
        self.set_variant_selector(cx, |context| {
            if context.screen_width < MIN_DESKTOP_WIDTH {
                live_id!(Mobile)
            } else {
                live_id!(Desktop)
            }
        });
    }
}

impl AdaptiveViewRef {
    /// Set a variant selector for this widget. 
    /// The selector is a closure that takes a `DisplayContext` and returns a `LiveId`, corresponding to the template to use.
    pub fn set_variant_selector(&mut self, cx: &mut Cx, selector: impl FnMut(&DisplayContext) -> LiveId + 'static) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.set_variant_selector(cx, selector);
    }
}

/// A closure that takes a `DisplayContext` and returns a `LiveId`, corresponding to the template to use.
pub type VariantSelector = dyn FnMut(&DisplayContext) -> LiveId;

/// A context that is used to determine which view to display in an `AdaptiveView` widget.
/// DisplayContext is stored in a global context so that they can be accessed from multiple `AdaptiveView` widget instances.
/// Later to be expanded with more context data like platfrom information, accessibility settings, etc.
#[derive(Clone, Debug)]
pub struct DisplayContext {
    pub updated_on_event_id: u64,
    pub screen_width: f64,
    /// The size of the parent obtained from running `cx.peek_walk_turtle(walk)` before the widget is drawn.
    pub parent_size: DVec2,
}

impl DisplayContext {
    pub fn is_desktop(&self) -> bool {
        self.screen_width >= MIN_DESKTOP_WIDTH
    }
}
