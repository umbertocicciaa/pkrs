//! Battle setup types owned by `pkrs-battle` (§7.2–§7.3).

/// Which side of the field an action or event refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SideId {
    /// The player's side (index 0).
    Player,
    /// The opposing side (index 1).
    Opponent,
}

impl SideId {
    /// The other side.
    pub fn other(self) -> Self {
        match self {
            Self::Player => Self::Opponent,
            Self::Opponent => Self::Player,
        }
    }
}

/// Active slot index on a side (`0..side_size`).
pub type SlotIndex = u8;

/// Index into a side's party bench.
pub type PartyIndex = u8;

/// Runtime battle configuration (§7.3 formats).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BattleConfig {
    /// Active slots per side (1 = singles, 2 = doubles plugin).
    pub side_size: u8,
}

impl Default for BattleConfig {
    fn default() -> Self {
        Self { side_size: 1 }
    }
}

impl BattleConfig {
    /// Validates `side_size` (1–6).
    pub fn validate(self) -> crate::Result<Self> {
        if (1..=6).contains(&self.side_size) {
            Ok(self)
        } else {
            Err(crate::BattleError::InvalidAction(format!(
                "side_size must be 1..=6 (got {})",
                self.side_size
            )))
        }
    }
}

/// Permanent battle stats for one creature (IV/EV folded in at setup time).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatureStats {
    /// Current HP (starts at `max_hp`).
    pub max_hp: u32,
    /// Attack (physical).
    pub attack: u32,
    /// Defense (physical).
    pub defense: u32,
    /// Speed (turn order).
    pub speed: u32,
}

impl CreatureStats {
    /// Creates stats with full HP.
    pub fn new(max_hp: u32, attack: u32, defense: u32, speed: u32) -> Self {
        Self {
            max_hp,
            attack,
            defense,
            speed,
        }
    }
}

/// One move slot on a creature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveSlot {
    /// Content id (e.g. `tackle`).
    pub id: String,
    /// Move priority for ordering (higher acts first).
    pub priority: i8,
    /// Base power for the stub damage formula (full formula is a later issue).
    pub power: u16,
    /// Remaining PP.
    pub pp: u8,
    /// Maximum PP.
    pub max_pp: u8,
}

/// One creature in a party.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleCreature {
    /// Stable content id.
    pub species_id: String,
    /// Element types (one or two) for STAB and effectiveness.
    pub types: Vec<String>,
    /// Display name.
    pub nickname: String,
    /// Level (informational in this milestone).
    pub level: u8,
    /// Battle stats.
    pub stats: CreatureStats,
    /// Up to four moves (sparse slots allowed).
    pub moves: [Option<MoveSlot>; 4],
}

/// Runtime creature with mutable HP and status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveCreature {
    pub template: BattleCreature,
    pub current_hp: u32,
    /// At most one major status.
    pub major_status: Option<MajorStatus>,
    /// Volatile statuses with turns remaining (`0` = indefinite until cleared).
    pub volatiles: std::collections::BTreeMap<String, u8>,
}

/// Major status with optional turn counter (sleep).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MajorStatus {
    /// Status content id.
    pub id: String,
    /// Turns remaining for sleep-like statuses.
    pub counter: u8,
}

impl ActiveCreature {
    pub fn from_template(template: BattleCreature) -> Self {
        let current_hp = template.stats.max_hp;
        Self {
            template,
            current_hp,
            major_status: None,
            volatiles: std::collections::BTreeMap::new(),
        }
    }

    pub fn fainted(&self) -> bool {
        self.current_hp == 0
    }

    pub fn speed(&self) -> u32 {
        self.template.stats.speed
    }
}

/// One side's party and active slots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleSideSetup {
    /// Trainer or player label (for messages).
    pub name: String,
    /// Full party (bench + actives).
    pub party: Vec<BattleCreature>,
    /// Which party indices start in each slot (`None` = empty slot).
    pub leading: Vec<Option<PartyIndex>>,
}

/// Everything needed to construct a battle (decoupled from `pkrs-data` loaders).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleSetup {
    /// Player side.
    pub player: BattleSideSetup,
    /// Opponent side.
    pub opponent: BattleSideSetup,
}

/// Outcome of a finished battle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattleOutcome {
    /// The player won.
    PlayerWin,
    /// The opponent won.
    OpponentWin,
    /// `side` successfully ran from battle.
    Fled {
        /// Who ran.
        side: SideId,
    },
}

/// Final battle summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BattleResult {
    /// How the battle ended.
    pub outcome: BattleOutcome,
    /// Number of full turns completed.
    pub turns: u32,
}
