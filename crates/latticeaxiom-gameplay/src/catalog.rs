use std::{
    collections::{BTreeMap, BTreeSet},
    num::{NonZeroU8, NonZeroU32},
};

use crate::{
    BlockId, GameplayReject, ItemId, ItemRoleId, ItemStackV1, ItemTagId, ProcessId, RecipeId,
    ToolClassId, WorkstationId,
};

/// Hard limits for one compiled gameplay catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogLimits {
    /// Exact items.
    pub items: usize,
    /// Exact blocks.
    pub blocks: usize,
    /// Explicit tool definitions.
    pub tools: usize,
    /// Item-tag definitions.
    pub tags: usize,
    /// Total exact members across all tags.
    pub tag_members: usize,
    /// Frozen item roles.
    pub roles: usize,
    /// Recipes.
    pub recipes: usize,
    /// Workstation contracts.
    pub workstations: usize,
    /// Scheduled processes.
    pub processes: usize,
    /// Explicit fuel rules.
    pub fuel_rules: usize,
    /// Predicate nodes in one predicate.
    pub predicate_nodes: usize,
    /// Predicate nesting in one predicate.
    pub predicate_depth: usize,
    /// Ingredients in one shapeless recipe.
    pub shapeless_ingredients: usize,
    /// Cells in one shaped recipe.
    pub shaped_cells: usize,
}

impl Default for CatalogLimits {
    fn default() -> Self {
        Self {
            items: 4_096,
            blocks: 4_096,
            tools: 512,
            tags: 1_024,
            tag_members: 65_536,
            roles: 1_024,
            recipes: 2_048,
            workstations: 128,
            processes: 1_024,
            fuel_rules: 512,
            predicate_nodes: 256,
            predicate_depth: 32,
            shapeless_ingredients: 16,
            shaped_cells: 25,
        }
    }
}

impl CatalogLimits {
    fn validate(self) -> Result<Self, GameplayReject> {
        let values = [
            ("catalog_items", self.items),
            ("catalog_blocks", self.blocks),
            ("catalog_tools", self.tools),
            ("catalog_tags", self.tags),
            ("catalog_tag_members", self.tag_members),
            ("catalog_roles", self.roles),
            ("catalog_recipes", self.recipes),
            ("catalog_workstations", self.workstations),
            ("catalog_processes", self.processes),
            ("catalog_fuel_rules", self.fuel_rules),
            ("predicate_nodes", self.predicate_nodes),
            ("predicate_depth", self.predicate_depth),
            ("shapeless_ingredients", self.shapeless_ingredients),
            ("shaped_cells", self.shaped_cells),
        ];
        for (resource, value) in values {
            if value == 0 {
                return Err(GameplayReject::LimitExceeded {
                    resource,
                    limit: 1,
                    actual: 0,
                });
            }
        }
        Ok(self)
    }
}

/// Version-one exact item definition consumed by sandbox mechanics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDefinitionV1 {
    /// Concrete item registration.
    pub id: ItemId,
    /// Non-zero inclusive stack limit.
    pub stack_limit: NonZeroU32,
    /// Concrete block placed by one consumed item, when applicable.
    pub placement_block: Option<BlockId>,
    /// Maximum persistent durability for singleton tools, when applicable.
    pub durability: Option<NonZeroU32>,
}

/// Narrow typed mining requirement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolRequirementV1 {
    /// Required versioned tool class.
    pub class: ToolClassId,
    /// Inclusive minimum tier.
    pub minimum_tier: u8,
}

/// Narrow typed mining rule used by real break-progress consumers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MiningRuleV1 {
    /// Required deterministic work units.
    pub hardness: NonZeroU32,
    /// Optional explicit tool requirement.
    pub tool: Option<ToolRequirementV1>,
}

/// Version-one exact block gameplay definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockDefinitionV1 {
    /// Concrete block registration.
    pub id: BlockId,
    /// Narrow mining rule.
    pub mining: MiningRuleV1,
    /// Concrete non-empty drop stack.
    pub drop: ItemStackV1,
}

/// Explicit tool mechanic definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolDefinitionV1 {
    /// Singleton tool item.
    pub item: ItemId,
    /// Versioned tool class.
    pub class: ToolClassId,
    /// Tool tier.
    pub tier: u8,
    /// Deterministic mining work added per command.
    pub work_per_step: NonZeroU32,
    /// Initial and maximum durability.
    pub maximum_durability: NonZeroU32,
}

/// Version-one item tag after authority validation by semantic compilation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemTagDefinitionV1 {
    /// Versioned tag contract.
    pub id: ItemTagId,
    /// Exact concrete members; compile order is irrelevant.
    pub members: Box<[ItemId]>,
}

/// Closed, serializable item predicate used by recipes and process rules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemPredicateV1 {
    /// Match one concrete item.
    Exact(ItemId),
    /// Match a compiled semantic tag.
    InTag(ItemTagId),
    /// Match every child predicate.
    All(Box<[Self]>),
    /// Match at least one child predicate.
    Any(Box<[Self]>),
    /// Negate one child predicate.
    Not(Box<Self>),
}

/// Version-one frozen output role definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemRoleDefinitionV1 {
    /// Versioned role contract.
    pub id: ItemRoleId,
    /// Closed acceptance predicate.
    pub accepts: ItemPredicateV1,
}

/// Concrete role binding restored from the frozen semantic receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenItemRoleBindingV1 {
    /// Versioned role contract.
    pub role: ItemRoleId,
    /// Concrete exact item selected by the frozen world/profile receipt.
    pub item: ItemId,
}

/// One quantified predicate input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngredientV1 {
    /// Accepted exact/tag/predicate set.
    pub accepts: ItemPredicateV1,
    /// Required quantity.
    pub quantity: NonZeroU32,
}

/// Frozen role output requested by a recipe or process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleOutputV1 {
    /// Versioned role contract.
    pub role: ItemRoleId,
    /// Non-zero output quantity.
    pub quantity: NonZeroU32,
}

/// Closed recipe layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecipePatternV1 {
    /// Order-independent quantified ingredients.
    Shapeless {
        /// Required ingredients.
        ingredients: Box<[IngredientV1]>,
    },
    /// Exact rectangular grid, including required empty cells.
    Shaped {
        /// Non-zero width.
        width: NonZeroU8,
        /// Non-zero height.
        height: NonZeroU8,
        /// Row-major cells; `None` requires the selected slot to be empty.
        cells: Box<[Option<IngredientV1>]>,
    },
}

/// Version-one recipe registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipeDefinitionV1 {
    /// Versioned recipe identity.
    pub id: RecipeId,
    /// Optional required workstation.
    pub workstation: Option<WorkstationId>,
    /// Shaped or shapeless pattern.
    pub pattern: RecipePatternV1,
    /// Frozen-role output intent.
    pub output: RoleOutputV1,
}

/// Generic workstation contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkstationDefinitionV1 {
    /// Versioned workstation identity.
    pub id: WorkstationId,
}

/// Explicit fuel qualification rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuelRuleV1 {
    /// Closed admission predicate. Merely joining another broad tag grants no
    /// fuel behavior unless that predicate is referenced here.
    pub accepts: ItemPredicateV1,
    /// Deterministic burn ticks for one consumed item.
    pub burn_ticks: NonZeroU32,
}

/// Version-one scheduled workstation process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessDefinitionV1 {
    /// Versioned process identity.
    pub id: ProcessId,
    /// Required generic workstation contract.
    pub workstation: WorkstationId,
    /// Quantified process input.
    pub input: IngredientV1,
    /// Frozen-role concrete output intent.
    pub output: RoleOutputV1,
    /// Fixed authoritative duration.
    pub duration_ticks: NonZeroU32,
}

/// Uncompiled, order-independent gameplay catalog input.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GameplayCatalogSourceV1 {
    /// Exact item definitions.
    pub items: Vec<ItemDefinitionV1>,
    /// Exact block definitions.
    pub blocks: Vec<BlockDefinitionV1>,
    /// Explicit tools.
    pub tools: Vec<ToolDefinitionV1>,
    /// Compiled semantic tags.
    pub tags: Vec<ItemTagDefinitionV1>,
    /// Item role contracts.
    pub roles: Vec<ItemRoleDefinitionV1>,
    /// Frozen concrete bindings.
    pub bindings: Vec<FrozenItemRoleBindingV1>,
    /// Recipes.
    pub recipes: Vec<RecipeDefinitionV1>,
    /// Workstations.
    pub workstations: Vec<WorkstationDefinitionV1>,
    /// Scheduled processes.
    pub processes: Vec<ProcessDefinitionV1>,
    /// Explicit fuel qualification rules.
    pub fuel_rules: Vec<FuelRuleV1>,
}

/// Validated deterministic gameplay catalog.
///
/// This conformance implementation deliberately retains typed raw stable-ID
/// text keys as a reference oracle. Production registration may intern them,
/// provided stable identity and order-independent behavior remain identical.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameplayCatalog {
    pub(crate) items: BTreeMap<ItemId, ItemDefinitionV1>,
    pub(crate) blocks: BTreeMap<BlockId, BlockDefinitionV1>,
    pub(crate) tools: BTreeMap<ItemId, ToolDefinitionV1>,
    pub(crate) tags: BTreeMap<ItemTagId, BTreeSet<ItemId>>,
    pub(crate) roles: BTreeMap<ItemRoleId, ItemRoleDefinitionV1>,
    pub(crate) bindings: BTreeMap<ItemRoleId, ItemId>,
    pub(crate) recipes: BTreeMap<RecipeId, RecipeDefinitionV1>,
    pub(crate) workstations: BTreeSet<WorkstationId>,
    pub(crate) processes: BTreeMap<ProcessId, ProcessDefinitionV1>,
    pub(crate) fuel_rules: Box<[FuelRuleV1]>,
    pub(crate) limits: CatalogLimits,
}

impl GameplayCatalog {
    /// Compiles a catalog without depending on discovery order.
    ///
    /// All top-level and nested counts are preflighted before sorting, graph
    /// validation, or predicate traversal.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection for hard-limit overflow, duplicate identity,
    /// unknown reference, malformed recipe/predicate, or invalid frozen role.
    pub fn compile(
        source: GameplayCatalogSourceV1,
        limits: CatalogLimits,
    ) -> Result<Self, GameplayReject> {
        let limits = limits.validate()?;
        preflight_source(&source, limits)?;

        let items = collect_unique(source.items, "item", |value| &value.id)?;
        let blocks = collect_unique(source.blocks, "block", |value| &value.id)?;
        let tools = collect_unique(source.tools, "tool", |value| &value.item)?;
        let roles = collect_unique(source.roles, "item_role", |value| &value.id)?;
        let bindings = collect_bindings(source.bindings)?;
        let recipes = collect_unique(source.recipes, "recipe", |value| &value.id)?;
        let processes = collect_unique(source.processes, "process", |value| &value.id)?;
        let workstations = collect_workstations(source.workstations)?;
        let tags = collect_tags(source.tags)?;

        let catalog = Self {
            items,
            blocks,
            tools,
            tags,
            roles,
            bindings,
            recipes,
            workstations,
            processes,
            fuel_rules: source.fuel_rules.into_boxed_slice(),
            limits,
        };
        catalog.validate_references()?;
        Ok(catalog)
    }

    /// Returns an exact item definition.
    #[must_use]
    pub fn item(&self, id: &ItemId) -> Option<&ItemDefinitionV1> {
        self.items.get(id)
    }

    /// Returns an exact block definition.
    #[must_use]
    pub fn block(&self, id: &BlockId) -> Option<&BlockDefinitionV1> {
        self.blocks.get(id)
    }

    /// Returns an explicit tool definition.
    #[must_use]
    pub fn tool(&self, id: &ItemId) -> Option<&ToolDefinitionV1> {
        self.tools.get(id)
    }

    /// Returns a recipe.
    #[must_use]
    pub fn recipe(&self, id: &RecipeId) -> Option<&RecipeDefinitionV1> {
        self.recipes.get(id)
    }

    /// Returns a scheduled process.
    #[must_use]
    pub fn process(&self, id: &ProcessId) -> Option<&ProcessDefinitionV1> {
        self.processes.get(id)
    }

    /// Returns compiled item definitions in identity order.
    #[must_use]
    pub fn items(&self) -> &BTreeMap<ItemId, ItemDefinitionV1> {
        &self.items
    }

    /// Returns compiled block definitions in identity order.
    #[must_use]
    pub fn blocks(&self) -> &BTreeMap<BlockId, BlockDefinitionV1> {
        &self.blocks
    }

    /// Returns compiled tool definitions in identity order.
    #[must_use]
    pub fn tools(&self) -> &BTreeMap<ItemId, ToolDefinitionV1> {
        &self.tools
    }

    /// Returns compiled recipes in identity order.
    #[must_use]
    pub fn recipes(&self) -> &BTreeMap<RecipeId, RecipeDefinitionV1> {
        &self.recipes
    }

    /// Returns compiled workstation contracts in identity order.
    #[must_use]
    pub fn workstations(&self) -> &BTreeSet<WorkstationId> {
        &self.workstations
    }

    /// Tests a closed item predicate against a concrete item.
    #[must_use]
    pub fn matches(&self, predicate: &ItemPredicateV1, item: &ItemId) -> bool {
        match predicate {
            ItemPredicateV1::Exact(expected) => expected == item,
            ItemPredicateV1::InTag(tag) => self
                .tags
                .get(tag)
                .is_some_and(|members| members.contains(item)),
            ItemPredicateV1::All(children) => {
                children.iter().all(|child| self.matches(child, item))
            }
            ItemPredicateV1::Any(children) => {
                children.iter().any(|child| self.matches(child, item))
            }
            ItemPredicateV1::Not(child) => !self.matches(child, item),
        }
    }

    /// Resolves a role output to a concrete validated stack.
    ///
    /// # Errors
    ///
    /// Returns a frozen-role rejection if the binding is absent and a stack
    /// rejection if the output exceeds the concrete item's limit.
    pub fn resolve_output(&self, output: &RoleOutputV1) -> Result<ItemStackV1, GameplayReject> {
        let item =
            self.bindings
                .get(&output.role)
                .ok_or_else(|| GameplayReject::FrozenRoleRejected {
                    role: output.role.as_str().to_owned(),
                })?;
        let definition = self
            .items
            .get(item)
            .ok_or_else(|| GameplayReject::UnknownReference {
                kind: "item",
                id: item.as_str().to_owned(),
            })?;
        let stack = match definition.durability {
            Some(durability) if output.quantity.get() == 1 => {
                ItemStackV1::tool(item.clone(), durability.get())?
            }
            Some(_) => {
                return Err(GameplayReject::StatefulStackQuantity {
                    quantity: output.quantity.get(),
                });
            }
            None => ItemStackV1::plain(item.clone(), output.quantity.get())?,
        };
        self.validate_stack(&stack)?;
        Ok(stack)
    }

    /// Validates a concrete stack against its item definition.
    ///
    /// # Errors
    ///
    /// Returns an unknown-reference, stack-limit, or stateful-stack rejection.
    pub fn validate_stack(&self, stack: &ItemStackV1) -> Result<(), GameplayReject> {
        let definition =
            self.items
                .get(stack.item())
                .ok_or_else(|| GameplayReject::UnknownReference {
                    kind: "item",
                    id: stack.item().as_str().to_owned(),
                })?;
        if stack.quantity() > definition.stack_limit.get() {
            return Err(GameplayReject::StackLimitExceeded {
                item: stack.item().clone(),
                quantity: stack.quantity(),
                limit: definition.stack_limit.get(),
            });
        }
        match (definition.durability, stack.state()) {
            (None, crate::ItemStateV1::Plain) => {}
            (Some(maximum), crate::ItemStateV1::ToolDurability { remaining })
                if stack.quantity() == 1 && remaining.get() <= maximum.get() => {}
            (Some(maximum), crate::ItemStateV1::ToolDurability { remaining })
                if remaining.get() > maximum.get() =>
            {
                return Err(GameplayReject::DurabilityOutOfRange {
                    item: stack.item().clone(),
                    remaining: remaining.get(),
                    maximum: maximum.get(),
                });
            }
            _ => {
                return Err(GameplayReject::ItemStateMismatch {
                    item: stack.item().clone(),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn best_fuel_ticks(&self, item: &ItemId) -> Option<u32> {
        self.fuel_rules
            .iter()
            .filter(|rule| self.matches(&rule.accepts, item))
            .map(|rule| rule.burn_ticks.get())
            .max()
    }

    fn validate_references(&self) -> Result<(), GameplayReject> {
        for item in self.items.values() {
            if let Some(block) = &item.placement_block {
                require_key(&self.blocks, block, "block")?;
            }
            if item.durability.is_some() && item.stack_limit.get() != 1 {
                return Err(GameplayReject::InvalidRecipe {
                    reason: "durable items must have stack limit one",
                });
            }
        }
        for block in self.blocks.values() {
            self.validate_stack(&block.drop)?;
            if let Some(requirement) = &block.mining.tool
                && requirement.class.as_str().is_empty()
            {
                return Err(GameplayReject::InvalidRecipe {
                    reason: "tool class cannot be empty",
                });
            }
        }
        for tool in self.tools.values() {
            let item = require_key(&self.items, &tool.item, "item")?;
            if item.stack_limit.get() != 1 || item.durability != Some(tool.maximum_durability) {
                return Err(GameplayReject::InvalidRecipe {
                    reason: "tool definition must match singleton item durability",
                });
            }
        }
        for (tag, members) in &self.tags {
            for member in members {
                if !self.items.contains_key(member) {
                    return Err(GameplayReject::UnknownReference {
                        kind: "item_tag_member",
                        id: format!("{} -> {}", tag.as_str(), member.as_str()),
                    });
                }
            }
        }
        for role in self.roles.values() {
            validate_predicate(&role.accepts, self, self.limits)?;
            let binding =
                self.bindings
                    .get(&role.id)
                    .ok_or_else(|| GameplayReject::FrozenRoleRejected {
                        role: role.id.as_str().to_owned(),
                    })?;
            if !self.items.contains_key(binding) || !self.matches(&role.accepts, binding) {
                return Err(GameplayReject::FrozenRoleRejected {
                    role: role.id.as_str().to_owned(),
                });
            }
        }
        for role in self.bindings.keys() {
            require_key(&self.roles, role, "item_role")?;
        }
        for recipe in self.recipes.values() {
            validate_recipe(recipe, self, self.limits)?;
            let _resolved = self.resolve_output(&recipe.output)?;
            if let Some(workstation) = &recipe.workstation
                && !self.workstations.contains(workstation)
            {
                return Err(GameplayReject::UnknownReference {
                    kind: "workstation",
                    id: workstation.as_str().to_owned(),
                });
            }
        }
        for rule in &self.fuel_rules {
            validate_predicate(&rule.accepts, self, self.limits)?;
        }
        for process in self.processes.values() {
            if !self.workstations.contains(&process.workstation) {
                return Err(GameplayReject::UnknownReference {
                    kind: "workstation",
                    id: process.workstation.as_str().to_owned(),
                });
            }
            validate_predicate(&process.input.accepts, self, self.limits)?;
            let _resolved = self.resolve_output(&process.output)?;
        }
        Ok(())
    }
}

fn preflight_source(
    source: &GameplayCatalogSourceV1,
    limits: CatalogLimits,
) -> Result<(), GameplayReject> {
    let top = [
        ("catalog_items", source.items.len(), limits.items),
        ("catalog_blocks", source.blocks.len(), limits.blocks),
        ("catalog_tools", source.tools.len(), limits.tools),
        ("catalog_tags", source.tags.len(), limits.tags),
        ("catalog_roles", source.roles.len(), limits.roles),
        ("catalog_bindings", source.bindings.len(), limits.roles),
        ("catalog_recipes", source.recipes.len(), limits.recipes),
        (
            "catalog_workstations",
            source.workstations.len(),
            limits.workstations,
        ),
        (
            "catalog_processes",
            source.processes.len(),
            limits.processes,
        ),
        (
            "catalog_fuel_rules",
            source.fuel_rules.len(),
            limits.fuel_rules,
        ),
    ];
    for (resource, actual, limit) in top {
        ensure_limit(resource, actual, limit)?;
    }
    let tag_members = source.tags.iter().try_fold(0_usize, |total, tag| {
        total
            .checked_add(tag.members.len())
            .ok_or(GameplayReject::LimitExceeded {
                resource: "catalog_tag_members",
                limit: limits.tag_members,
                actual: usize::MAX,
            })
    })?;
    ensure_limit("catalog_tag_members", tag_members, limits.tag_members)?;

    for recipe in &source.recipes {
        match &recipe.pattern {
            RecipePatternV1::Shapeless { ingredients } => ensure_limit(
                "shapeless_ingredients",
                ingredients.len(),
                limits.shapeless_ingredients,
            )?,
            RecipePatternV1::Shaped { cells, .. } => {
                ensure_limit("shaped_cells", cells.len(), limits.shaped_cells)?;
            }
        }
    }
    Ok(())
}

fn ensure_limit(resource: &'static str, actual: usize, limit: usize) -> Result<(), GameplayReject> {
    if actual > limit {
        return Err(GameplayReject::LimitExceeded {
            resource,
            limit,
            actual,
        });
    }
    Ok(())
}

fn collect_unique<K, V, F>(
    values: Vec<V>,
    kind: &'static str,
    key: F,
) -> Result<BTreeMap<K, V>, GameplayReject>
where
    K: Clone + Ord + ToString,
    F: Fn(&V) -> &K,
{
    let mut result = BTreeMap::new();
    for value in values {
        let id = key(&value).clone();
        if result.insert(id.clone(), value).is_some() {
            return Err(GameplayReject::DuplicateRegistration {
                kind,
                id: id.to_string(),
            });
        }
    }
    Ok(result)
}

fn collect_bindings(
    values: Vec<FrozenItemRoleBindingV1>,
) -> Result<BTreeMap<ItemRoleId, ItemId>, GameplayReject> {
    let mut result = BTreeMap::new();
    for value in values {
        if result.insert(value.role.clone(), value.item).is_some() {
            return Err(GameplayReject::DuplicateRegistration {
                kind: "item_role_binding",
                id: value.role.as_str().to_owned(),
            });
        }
    }
    Ok(result)
}

fn collect_workstations(
    values: Vec<WorkstationDefinitionV1>,
) -> Result<BTreeSet<WorkstationId>, GameplayReject> {
    let mut result = BTreeSet::new();
    for value in values {
        if !result.insert(value.id.clone()) {
            return Err(GameplayReject::DuplicateRegistration {
                kind: "workstation",
                id: value.id.as_str().to_owned(),
            });
        }
    }
    Ok(result)
}

fn collect_tags(
    values: Vec<ItemTagDefinitionV1>,
) -> Result<BTreeMap<ItemTagId, BTreeSet<ItemId>>, GameplayReject> {
    let mut result = BTreeMap::new();
    for value in values {
        let members = value.members.into_vec().into_iter().collect();
        if result.insert(value.id.clone(), members).is_some() {
            return Err(GameplayReject::DuplicateRegistration {
                kind: "item_tag",
                id: value.id.as_str().to_owned(),
            });
        }
    }
    Ok(result)
}

fn require_key<'a, K, V>(
    map: &'a BTreeMap<K, V>,
    key: &K,
    kind: &'static str,
) -> Result<&'a V, GameplayReject>
where
    K: Ord + ToString,
{
    map.get(key)
        .ok_or_else(|| GameplayReject::UnknownReference {
            kind,
            id: key.to_string(),
        })
}

fn validate_predicate(
    predicate: &ItemPredicateV1,
    catalog: &GameplayCatalog,
    limits: CatalogLimits,
) -> Result<(), GameplayReject> {
    let mut stack = vec![(predicate, 1_usize)];
    let mut nodes = 0_usize;
    while let Some((current, depth)) = stack.pop() {
        nodes = nodes
            .checked_add(1)
            .ok_or(GameplayReject::InvalidPredicate {
                reason: "predicate node count overflow",
            })?;
        ensure_limit("predicate_nodes", nodes, limits.predicate_nodes)?;
        ensure_limit("predicate_depth", depth, limits.predicate_depth)?;
        match current {
            ItemPredicateV1::Exact(item) => {
                require_key(&catalog.items, item, "item")?;
            }
            ItemPredicateV1::InTag(tag) => {
                require_key(&catalog.tags, tag, "item_tag")?;
            }
            ItemPredicateV1::All(children) | ItemPredicateV1::Any(children) => {
                if children.is_empty() {
                    return Err(GameplayReject::InvalidPredicate {
                        reason: "All and Any predicates require at least one child",
                    });
                }
                for child in children.iter().rev() {
                    let child_depth =
                        depth
                            .checked_add(1)
                            .ok_or(GameplayReject::InvalidPredicate {
                                reason: "predicate depth overflow",
                            })?;
                    stack.push((child, child_depth));
                }
            }
            ItemPredicateV1::Not(child) => {
                let child_depth = depth
                    .checked_add(1)
                    .ok_or(GameplayReject::InvalidPredicate {
                        reason: "predicate depth overflow",
                    })?;
                stack.push((child, child_depth));
            }
        }
    }
    Ok(())
}

fn validate_recipe(
    recipe: &RecipeDefinitionV1,
    catalog: &GameplayCatalog,
    limits: CatalogLimits,
) -> Result<(), GameplayReject> {
    match &recipe.pattern {
        RecipePatternV1::Shapeless { ingredients } => {
            if ingredients.is_empty() {
                return Err(GameplayReject::InvalidRecipe {
                    reason: "shapeless recipe requires at least one ingredient",
                });
            }
            ensure_limit(
                "shapeless_ingredients",
                ingredients.len(),
                limits.shapeless_ingredients,
            )?;
            for ingredient in ingredients {
                validate_predicate(&ingredient.accepts, catalog, limits)?;
            }
        }
        RecipePatternV1::Shaped {
            width,
            height,
            cells,
        } => {
            let expected = usize::from(width.get())
                .checked_mul(usize::from(height.get()))
                .ok_or(GameplayReject::InvalidRecipe {
                    reason: "shaped recipe dimensions overflow",
                })?;
            if cells.len() != expected {
                return Err(GameplayReject::InvalidRecipe {
                    reason: "shaped recipe cell count does not match width times height",
                });
            }
            ensure_limit("shaped_cells", cells.len(), limits.shaped_cells)?;
            if cells.iter().all(Option::is_none) {
                return Err(GameplayReject::InvalidRecipe {
                    reason: "shaped recipe cannot be entirely empty",
                });
            }
            for ingredient in cells.iter().flatten() {
                validate_predicate(&ingredient.accepts, catalog, limits)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::{
        GameplayCatalog, GameplayCatalogSourceV1, GameplayReject, ItemDefinitionV1, ItemId,
    };

    #[test]
    fn catalog_limit_is_checked_before_collection() {
        let item = ItemDefinitionV1 {
            id: ItemId::parse("example:item/a").expect("fixture ID is canonical"),
            stack_limit: NonZeroU32::new(64).expect("fixture limit is non-zero"),
            placement_block: None,
            durability: None,
        };
        let source = GameplayCatalogSourceV1 {
            items: vec![item.clone(), item],
            ..GameplayCatalogSourceV1::default()
        };
        let limits = super::CatalogLimits {
            items: 1,
            ..super::CatalogLimits::default()
        };
        assert!(matches!(
            GameplayCatalog::compile(source, limits),
            Err(GameplayReject::LimitExceeded {
                resource: "catalog_items",
                actual: 2,
                ..
            })
        ));
    }
}
