//! Inventory, hand-crafting, and workbench overlays. Draft state is not authority.

use std::collections::BTreeSet;

use crate::semantic::{SemanticKey, SemanticNode, SemanticRole, SemanticState};
use crate::views::keys::{surface_key, try_surface_key};
use crate::widgets::{ButtonWidget, ListItemWidget, ListWidget};

/// Inventory container size. Hotbar is the prefix `0..8`.
pub const INVENTORY_SLOT_COUNT: u16 = 36;

/// One inventory slot projection. Labels are display text, not content IDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventorySlotV1 {
    /// Slot index in `0..INVENTORY_SLOT_COUNT`.
    pub index: u16,
    /// Display label, empty when unoccupied.
    pub label: String,
    /// Stack quantity when occupied.
    pub quantity: Option<u32>,
}

impl InventorySlotV1 {
    /// Returns whether the slot is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.label.is_empty() && self.quantity.is_none()
    }

    fn key(&self) -> Result<SemanticKey, crate::SemanticKeyError> {
        try_surface_key(format!("overlay/inventory/slot-{}", self.index))
    }

    fn semantic_node(
        &self,
        latched: bool,
        focused: bool,
        interactive: bool,
    ) -> Result<SemanticNode, crate::SemanticKeyError> {
        let name = if self.label.is_empty() {
            format!("Empty slot {}", self.index)
        } else {
            self.label.clone()
        };
        let mut node = ButtonWidget::new(
            self.key()?,
            name,
            Some(if latched {
                "latched click-to-swap source".to_owned()
            } else {
                "inventory slot".to_owned()
            }),
            interactive,
        )
        .semantic_node(focused);
        node.value = self.quantity.map(|quantity| quantity.to_string());
        Ok(node)
    }
}

/// Result of a presentation click. Authority changes only via a `MoveStack` receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InventoryClickV1 {
    /// First click stored `slot` as the cursor latch.
    Latched(u16),
    /// Second click on the same slot cleared the latch.
    Cleared,
    /// Second click produced a host-bound move intent. Slots are unchanged.
    RequestMove {
        /// Source slot.
        from: u16,
        /// Destination slot.
        to: u16,
    },
}

/// Overlay-owned click-to-swap draft. It cannot write inventory slots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryDraftV1 {
    cursor_slot: Option<u16>,
    slots: Vec<InventorySlotV1>,
}

impl InventoryDraftV1 {
    /// Empty 36-slot draft.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            cursor_slot: None,
            slots: (0..INVENTORY_SLOT_COUNT)
                .map(|index| InventorySlotV1 {
                    index,
                    label: String::new(),
                    quantity: None,
                })
                .collect(),
        }
    }

    /// Replaces slot visuals from an authoritative snapshot without clearing the latch
    /// unless the latched slot vanished.
    pub fn sync_slots(&mut self, slots: Vec<InventorySlotV1>) {
        self.slots = slots;
        if let Some(cursor) = self.cursor_slot
            && usize::from(cursor) >= self.slots.len()
        {
            self.cursor_slot = None;
        }
    }

    /// Returns the presentation latch. This is not an input-context latch.
    #[must_use]
    pub const fn cursor_slot(&self) -> Option<u16> {
        self.cursor_slot
    }

    /// Returns slots in index order.
    #[must_use]
    pub fn slots(&self) -> &[InventorySlotV1] {
        &self.slots
    }

    /// Click-to-swap. Never mutates slot contents.
    pub fn click_slot(&mut self, slot: u16) -> InventoryClickV1 {
        match self.cursor_slot {
            None => {
                self.cursor_slot = Some(slot);
                InventoryClickV1::Latched(slot)
            }
            Some(from) if from == slot => {
                self.cursor_slot = None;
                InventoryClickV1::Cleared
            }
            Some(from) => {
                self.cursor_slot = None;
                InventoryClickV1::RequestMove { from, to: slot }
            }
        }
    }

    /// Clears the latch when the overlay closes.
    pub fn clear_latch(&mut self) {
        self.cursor_slot = None;
    }

    /// Projects the inventory grid.
    ///
    /// # Errors
    ///
    /// Returns [`crate::SemanticKeyError`] if a generated slot key is invalid.
    pub fn semantic_node(
        &self,
        focused: Option<&SemanticKey>,
        interactive: bool,
    ) -> Result<SemanticNode, crate::SemanticKeyError> {
        let children = self
            .slots
            .iter()
            .map(|slot| {
                slot.semantic_node(
                    self.cursor_slot == Some(slot.index),
                    focused == Some(&slot.key()?) && interactive,
                    interactive,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SemanticNode {
            key: surface_key("overlay/inventory"),
            role: SemanticRole::Group,
            name: "Inventory".to_owned(),
            value: self.cursor_slot.map(|slot| slot.to_string()),
            description: Some(
                "Click-to-swap draft. Authoritative inventory changes require a MoveStack receipt."
                    .to_owned(),
            ),
            state: SemanticState {
                focusable: false,
                focused: false,
                disabled: false,
                expanded: Some(true),
                busy: false,
            },
            actions: BTreeSet::new(),
            children,
        })
    }
}

/// Catalog recipe row. The client does not match a 3×3 grid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipeRowV1 {
    /// Stable recipe identity text. Callers must not hard-code dimension IDs.
    pub id: String,
    /// Localized display name.
    pub name: String,
    /// Whether inputs currently owned make the recipe craftable.
    pub craftable: bool,
}

impl RecipeRowV1 {
    fn key(&self, workbench: bool) -> Result<SemanticKey, crate::SemanticKeyError> {
        let prefix = if workbench {
            "overlay/workbench/recipe"
        } else {
            "overlay/inventory/recipe"
        };
        let sanitized: String = self
            .id
            .chars()
            .map(|character| {
                if character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '/' | '-' | '_' | ':')
                {
                    character
                } else {
                    '-'
                }
            })
            .collect();
        try_surface_key(format!("{prefix}/{sanitized}"))
    }
}

/// Recipe list rebuilt from the locked catalog each sync.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecipeListV1 {
    /// Rows in catalog identity order.
    pub recipes: Vec<RecipeRowV1>,
    /// Selected recipe identity, when present.
    pub selected: Option<String>,
}

impl RecipeListV1 {
    /// Projects a recipe list. Crafting submits a catalog command; it does not guess a grid.
    ///
    /// # Errors
    ///
    /// Returns [`crate::SemanticKeyError`] if a recipe identity cannot be a semantic key.
    pub fn semantic_node(
        &self,
        workbench: bool,
        focused: Option<&SemanticKey>,
        interactive: bool,
    ) -> Result<SemanticNode, crate::SemanticKeyError> {
        let items = self
            .recipes
            .iter()
            .map(|recipe| {
                Ok(ListItemWidget {
                    key: recipe.key(workbench)?,
                    name: recipe.name.clone(),
                    value: Some(if recipe.craftable {
                        "craftable".to_owned()
                    } else {
                        "unavailable".to_owned()
                    }),
                    enabled: recipe.craftable && interactive,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let selected = self.selected.as_ref().and_then(|id| {
            self.recipes
                .iter()
                .find(|recipe| &recipe.id == id)
                .and_then(|recipe| recipe.key(workbench).ok())
        });
        let list = ListWidget {
            key: surface_key(if workbench {
                "overlay/workbench/recipes"
            } else {
                "overlay/inventory/recipes"
            }),
            name: if workbench {
                "Workbench recipes".to_owned()
            } else {
                "Hand crafting".to_owned()
            },
            items,
            selected,
        };
        Ok(list.semantic_node(focused))
    }
}

/// Workbench overlay bound to a workstation identity from the catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkbenchOverlayV1 {
    /// Workstation identity text from the locked catalog, not a host dimension ID.
    pub workstation: String,
    /// Recipes that require this workstation.
    pub recipes: RecipeListV1,
}

impl WorkbenchOverlayV1 {
    /// Empty workbench with no recipes.
    #[must_use]
    pub fn empty(workstation: impl Into<String>) -> Self {
        Self {
            workstation: workstation.into(),
            recipes: RecipeListV1::default(),
        }
    }

    /// Projects the workbench overlay.
    ///
    /// # Errors
    ///
    /// Returns [`crate::SemanticKeyError`] if a recipe identity cannot be a semantic key.
    pub fn semantic_node(
        &self,
        focused: Option<&SemanticKey>,
        interactive: bool,
    ) -> Result<SemanticNode, crate::SemanticKeyError> {
        Ok(SemanticNode {
            key: surface_key("overlay/workbench"),
            role: SemanticRole::Group,
            name: "Workbench".to_owned(),
            value: Some(self.workstation.clone()),
            description: Some(
                "Catalog recipes for the bound workstation. Right-click place is not stolen."
                    .to_owned(),
            ),
            state: SemanticState {
                focusable: false,
                focused: false,
                disabled: !interactive,
                expanded: Some(true),
                busy: false,
            },
            actions: BTreeSet::new(),
            children: vec![self.recipes.semantic_node(
                true,
                if interactive { focused } else { None },
                interactive,
            )?],
        })
    }
}
