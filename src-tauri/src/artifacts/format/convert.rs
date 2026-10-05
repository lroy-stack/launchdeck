//! A surface of the workspace as a file carries it, and back.
//!
//! Looks are the workspace's `states`. Never carried: a control's legacy
//! bindings and a surface's context hint. Assignments are carried as they
//! are; what export strips or adds (legacy actions, input bindings, surface
//! references) is its own decision.

use serde_json::Value;

use super::content::{
    AnimationV1, AssignmentV1, Color, ControlV1, PageV1, Pos, Span, SurfaceV1, VisualV1,
};
use super::rejection::echo;
use crate::core::model::{self, Animation, Assignment, Control, ControlPos, Page, Surface, Visual};

impl SurfaceV1 {
    /// The surface as a file carries it. Fails only for a control on the
    /// logo LED, which a valid workspace never has.
    pub fn from_model(surface: &Surface) -> Result<SurfaceV1, String> {
        Ok(SurfaceV1 {
            id: surface.id.clone(),
            name: surface.name.clone(),
            description: surface.description.clone(),
            icon: icon(&surface.icon),
            category: surface.category.clone(),
            pages: surface
                .pages
                .iter()
                .map(page_from_model)
                .collect::<Result<_, _>>()?,
        })
    }

    pub fn to_model(&self) -> Surface {
        Surface {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            icon: self.icon.clone(),
            category: self.category.clone(),
            pages: self.pages.iter().map(page_to_model).collect(),
            context: None,
            imported_from: None,
        }
    }
}

/// An empty icon is no icon.
fn icon(icon: &Option<String>) -> Option<String> {
    icon.clone().filter(|i| !i.is_empty())
}

fn page_from_model(page: &Page) -> Result<PageV1, String> {
    Ok(PageV1 {
        id: page.id.clone(),
        name: page.name.clone(),
        controls: page
            .controls
            .iter()
            .map(control_from_model)
            .collect::<Result<_, _>>()?,
    })
}

pub(super) fn page_to_model(page: &PageV1) -> Page {
    Page {
        id: page.id.clone(),
        name: page.name.clone(),
        controls: page.controls.iter().map(control_to_model).collect(),
    }
}

fn control_from_model(c: &Control) -> Result<ControlV1, String> {
    let pos = match c.pos {
        ControlPos::Grid { col, row } => Pos::Grid { col, row },
        ControlPos::Top { index } => Pos::Top { index },
        ControlPos::Side { index } => Pos::Side { index },
        ControlPos::Logo => {
            return Err(format!(
                "The control \"{}\" is on the logo LED, which cannot hold a control.",
                echo(&c.label)
            ))
        }
    };
    let span = (c.span != model::Span::default()).then_some(Span {
        w: c.span.w,
        h: c.span.h,
    });
    Ok(ControlV1 {
        id: c.id.clone(),
        pos,
        span,
        label: c.label.clone(),
        description: c.description.clone(),
        icon: icon(&c.icon),
        visual: visual_from_model(c.visual),
        looks: c
            .states
            .iter()
            .map(|(name, v)| (name.clone(), visual_from_model(*v)))
            .collect(),
        press: c.press.as_ref().map(AssignmentV1::from_model),
        long_press: c.long_press.as_ref().map(AssignmentV1::from_model),
        feedback: c.feedback.clone(),
    })
}

fn control_to_model(c: &ControlV1) -> Control {
    let pos = match c.pos {
        Pos::Grid { col, row } => ControlPos::Grid { col, row },
        Pos::Top { index } => ControlPos::Top { index },
        Pos::Side { index } => ControlPos::Side { index },
    };
    Control {
        id: c.id.clone(),
        pos,
        span: c
            .span
            .map_or_else(model::Span::default, |s| model::Span { w: s.w, h: s.h }),
        label: c.label.clone(),
        description: c.description.clone(),
        icon: c.icon.clone(),
        visual: visual_to_model(c.visual),
        states: c
            .looks
            .iter()
            .map(|(name, v)| (name.clone(), visual_to_model(*v)))
            .collect(),
        press: c.press.as_ref().map(AssignmentV1::to_model),
        long_press: c.long_press.as_ref().map(AssignmentV1::to_model),
        feedback: c.feedback.clone(),
        legacy_bindings: Vec::new(),
    }
}

fn visual_from_model(v: Visual) -> VisualV1 {
    VisualV1 {
        color: Color(v.color),
        animation: match v.animation {
            Animation::None => AnimationV1::None,
            Animation::Pulse => AnimationV1::Pulse,
            Animation::Flash => AnimationV1::Flash,
        },
    }
}

fn visual_to_model(v: VisualV1) -> Visual {
    Visual {
        color: v.color.0,
        animation: match v.animation {
            AnimationV1::None => Animation::None,
            AnimationV1::Pulse => Animation::Pulse,
            AnimationV1::Flash => Animation::Flash,
        },
    }
}

impl AssignmentV1 {
    /// The assignment as it is, with no input binding or surface reference.
    pub fn from_model(a: &Assignment) -> AssignmentV1 {
        AssignmentV1 {
            provider: a.provider.clone(),
            action: a.action.clone(),
            args: match &a.args {
                Value::Object(map) => map.clone(),
                _ => serde_json::Map::new(),
            },
            inputs: Default::default(),
            surface_refs: Vec::new(),
        }
    }

    pub fn to_model(&self) -> Assignment {
        Assignment::new(
            &self.provider,
            &self.action,
            Value::Object(self.args.clone()),
        )
    }
}
