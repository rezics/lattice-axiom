//! High-frequency HUD: status, hotbar, and target inspect. Occupancy stays off this overlay.

use std::collections::BTreeSet;

use latticeaxiom_runtime_contracts::{
    PresentedRenderDistanceChunksV1, TargetRenderDistanceChunksV1,
};

use crate::semantic::{SemanticKey, SemanticNode, SemanticRole, SemanticState};
use crate::views::keys::{surface_key, try_surface_key};
use crate::widgets::ButtonWidget;

/// Hotbar prefix of the 36-slot inventory container.
pub const HUD_HOTBAR_SLOTS: u8 = 9;

/// Presentation-neutral vitality, mining, durability, and render-distance strip.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HudStatusV1 {
    /// Current vitality points.
    pub vitality_current: u32,
    /// Maximum vitality points.
    pub vitality_max: u32,
    /// Remaining mining work, when a break is in progress.
    pub mining_remaining: Option<u32>,
    /// Remaining durability of the selected tool, when present.
    pub tool_durability: Option<u32>,
    /// Total terrain horizon admitted by the active host.
    pub target_render_distance: TargetRenderDistanceChunksV1,
    /// Largest contiguous near-plus-far radius ready for presentation.
    pub presented_render_distance: PresentedRenderDistanceChunksV1,
}

impl HudStatusV1 {
    /// Formats the status strip without occupancy counts.
    #[must_use]
    pub fn line(&self) -> String {
        let mine = self
            .mining_remaining
            .map_or_else(|| "Mine —".to_owned(), |left| format!("Mine {left} left"));
        let tool = self
            .tool_durability
            .map_or_else(|| "Tool —".to_owned(), |left| format!("Tool {left}"));
        format!(
            "Vitality {}/{}  {mine}  {tool}  Render {}/{}",
            self.vitality_current,
            self.vitality_max,
            self.presented_render_distance.chunks(),
            self.target_render_distance.chunks()
        )
    }

    /// Projects a non-focusable status node.
    #[must_use]
    pub fn semantic_node(&self) -> SemanticNode {
        SemanticNode {
            key: surface_key("hud/status"),
            role: SemanticRole::Status,
            name: "Player status".to_owned(),
            value: Some(self.line()),
            description: Some(
                "Vitality, mining progress, tool durability, and presented render distance"
                    .to_owned(),
            ),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children: Vec::new(),
        }
    }
}

/// One hotbar slot. Hotbar is a window into inventory, not a second authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HotbarSlotV1 {
    /// Slot index in `0..HUD_HOTBAR_SLOTS`.
    pub index: u8,
    /// Display label for the stack, or empty.
    pub label: String,
    /// Stack quantity when occupied.
    pub quantity: Option<u32>,
    /// Whether this is the selected hotbar slot.
    pub selected: bool,
}

impl HotbarSlotV1 {
    fn key(&self) -> Result<SemanticKey, crate::SemanticKeyError> {
        try_surface_key(format!("hud/hotbar/slot-{}", self.index))
    }

    fn semantic_node(
        &self,
        interactive: bool,
        focused: bool,
    ) -> Result<SemanticNode, crate::SemanticKeyError> {
        let key = self.key()?;
        let name = if self.label.is_empty() {
            format!("Hotbar {}", self.index.saturating_add(1))
        } else {
            self.label.clone()
        };
        let value = self.quantity.map(|quantity| quantity.to_string());
        Ok(if interactive {
            ButtonWidget::new(
                key,
                name,
                Some(if self.selected {
                    "selected hotbar slot".to_owned()
                } else {
                    "hotbar slot".to_owned()
                }),
                true,
            )
            .semantic_node(focused)
        } else {
            SemanticNode {
                key,
                role: SemanticRole::Status,
                name,
                value,
                description: Some(if self.selected {
                    "selected hotbar slot".to_owned()
                } else {
                    "hotbar slot".to_owned()
                }),
                state: SemanticState::default(),
                actions: BTreeSet::new(),
                children: Vec::new(),
            }
        })
    }
}

/// Nine-slot hotbar projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HotbarV1 {
    /// Slots in index order.
    pub slots: Vec<HotbarSlotV1>,
}

impl HotbarV1 {
    /// Empty hotbar with slot 0 selected.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            slots: (0..HUD_HOTBAR_SLOTS)
                .map(|index| HotbarSlotV1 {
                    index,
                    label: String::new(),
                    quantity: None,
                    selected: index == 0,
                })
                .collect(),
        }
    }

    /// Projects the hotbar. Slots become buttons only while an overlay is open.
    ///
    /// # Errors
    ///
    /// Returns [`crate::SemanticKeyError`] if a generated slot key is invalid.
    pub fn semantic_node(
        &self,
        interactive: bool,
        focused: Option<&SemanticKey>,
    ) -> Result<SemanticNode, crate::SemanticKeyError> {
        let children = self
            .slots
            .iter()
            .map(|slot| slot.semantic_node(interactive, focused == Some(&slot.key()?)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SemanticNode {
            key: surface_key("hud/hotbar"),
            role: SemanticRole::Group,
            name: "Hotbar".to_owned(),
            value: self
                .slots
                .iter()
                .find(|slot| slot.selected)
                .map(|slot| slot.index.to_string()),
            description: Some("Selected window into the inventory container".to_owned()),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children,
        })
    }
}

/// Player-facing target inspect. Occupancy is not a fragment on this overlay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectOverlayV1 {
    /// `header.name`
    pub name: String,
    /// `header.icon` identity (color swatch until PNG).
    pub icon: String,
    /// `summary.harvestability`
    pub harvestability: String,
    /// `technical.declared-by` package namespace.
    pub declared_by: String,
    /// `technical.stable-id`
    pub stable_id: String,
}

impl InspectOverlayV1 {
    /// Empty overlay used when DDA misses.
    #[must_use]
    pub fn none() -> Self {
        Self {
            name: "Inspect — no target".to_owned(),
            icon: String::new(),
            harvestability: String::new(),
            declared_by: String::new(),
            stable_id: String::new(),
        }
    }

    /// Player overlay lines. Occupancy and chunk coordinates are omitted.
    #[must_use]
    pub fn overlay_lines(&self) -> String {
        if self.stable_id.is_empty() {
            self.name.clone()
        } else {
            format!(
                "{}\n{}\ndeclared-by {}\n{}",
                self.name, self.harvestability, self.declared_by, self.stable_id
            )
        }
    }

    /// Returns whether occupancy leaked into the player overlay.
    #[must_use]
    pub fn contains_occupancy(&self) -> bool {
        let haystack = format!(
            "{} {} {} {}",
            self.name, self.harvestability, self.declared_by, self.stable_id
        );
        haystack.split_whitespace().any(|token| {
            token.starts_with('r') && token.chars().nth(1).is_some_and(|ch| ch.is_ascii_digit())
                || token.starts_with("occupancy")
        })
    }

    /// Projects typed inspect fragments in stable key order.
    #[must_use]
    pub fn semantic_node(&self) -> SemanticNode {
        let children = [
            ("hud/inspect/header-name", "Name", self.name.as_str()),
            ("hud/inspect/header-icon", "Icon", self.icon.as_str()),
            (
                "hud/inspect/harvestability",
                "Harvestability",
                self.harvestability.as_str(),
            ),
            (
                "hud/inspect/declared-by",
                "Declared by",
                self.declared_by.as_str(),
            ),
            (
                "hud/inspect/stable-id",
                "Stable id",
                self.stable_id.as_str(),
            ),
        ]
        .into_iter()
        .map(|(key, name, value)| SemanticNode {
            key: surface_key(key),
            role: SemanticRole::Status,
            name: name.to_owned(),
            value: (!value.is_empty()).then(|| value.to_owned()),
            description: None,
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children: Vec::new(),
        })
        .collect();
        SemanticNode {
            key: surface_key("hud/inspect"),
            role: SemanticRole::Status,
            name: "Target inspect".to_owned(),
            value: Some(self.overlay_lines()),
            description: Some(
                "Always-on target overlay. Occupancy remains on the working-set path.".to_owned(),
            ),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children,
        }
    }
}

/// Combined HUD model bound to the current route epoch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HudModelV1 {
    /// Status strip.
    pub status: HudStatusV1,
    /// Hotbar window.
    pub hotbar: HotbarV1,
    /// Target inspect overlay.
    pub inspect: InspectOverlayV1,
}

impl HudModelV1 {
    /// Default empty HUD.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            status: HudStatusV1 {
                vitality_current: 20,
                vitality_max: 20,
                mining_remaining: None,
                tool_durability: None,
                target_render_distance: TargetRenderDistanceChunksV1::default(),
                presented_render_distance: PresentedRenderDistanceChunksV1::default(),
            },
            hotbar: HotbarV1::empty(),
            inspect: InspectOverlayV1::none(),
        }
    }

    /// Projects HUD children. Hotbar is interactive only with an overlay open.
    ///
    /// # Errors
    ///
    /// Returns [`crate::SemanticKeyError`] if a generated key is invalid.
    pub fn semantic_children(
        &self,
        hotbar_interactive: bool,
        focused: Option<&SemanticKey>,
    ) -> Result<Vec<SemanticNode>, crate::SemanticKeyError> {
        Ok(vec![
            self.status.semantic_node(),
            self.inspect.semantic_node(),
            self.hotbar.semantic_node(hotbar_interactive, focused)?,
        ])
    }
}
