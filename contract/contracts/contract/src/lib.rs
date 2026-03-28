#![allow(non_snake_case)]
#![no_std]
use soroban_sdk::{contract, contracttype, contractimpl, log, Env, Symbol, String, symbol_short};

// Tracks global portfolio mirroring statistics across the platform
#[contracttype]
#[derive(Clone)]
pub struct MirrorStats {
    pub total_portfolios: u64,   // Total master portfolios ever registered
    pub total_mirrors: u64,      // Total mirror subscriptions ever created
    pub active_mirrors: u64,     // Currently active mirror subscriptions
    pub total_syncs: u64,        // Total sync operations performed across all mirrors
}

// Reference key for global stats
const ALL_STATS: Symbol = symbol_short!("ALL_STATS");

// Maps portfolio_id -> Portfolio struct
#[contracttype]
pub enum Portfoliobook {
    Portfolio(u64),
}

// Maps mirror_id -> MirrorEntry struct
#[contracttype]
pub enum Mirrorbook {
    Mirror(u64),
}

// Counters for unique ID generation
const COUNT_PORTFOLIO: Symbol = symbol_short!("C_PORT");
const COUNT_MIRROR:    Symbol = symbol_short!("C_MIR");

// Represents a master portfolio published by an expert/leader
#[contracttype]
#[derive(Clone)]
pub struct Portfolio {
    pub portfolio_id: u64,       // Unique ID of this portfolio
    pub owner_alias: String,     // On-chain alias/name of the portfolio owner
    pub strategy_desc: String,   // Short description of the investment strategy
    pub asset_allocation: String,// Encoded allocation string e.g. "BTC:40,ETH:35,XLM:25"
    pub created_at: u64,         // Ledger timestamp of creation
    pub last_updated: u64,       // Ledger timestamp of last allocation update
    pub mirror_count: u64,       // Number of wallets currently mirroring this portfolio
    pub is_active: bool,         // Whether this portfolio is open for mirroring
}

// Represents a follower's mirror subscription of a master portfolio
#[contracttype]
#[derive(Clone)]
pub struct MirrorEntry {
    pub mirror_id: u64,          // Unique ID of this mirror subscription
    pub portfolio_id: u64,       // The master portfolio being mirrored
    pub follower_alias: String,  // Alias of the follower wallet
    pub synced_allocation: String,// Last allocation synced from the master
    pub subscribed_at: u64,      // Ledger timestamp when mirroring started
    pub last_sync: u64,          // Ledger timestamp of last sync
    pub sync_count: u64,         // How many times this mirror has been synced
    pub is_active: bool,         // Whether this mirror is still active
}

#[contract]
pub struct PortfolioMirroringContract;

#[contractimpl]
impl PortfolioMirroringContract {

    /// Register a new master portfolio that others can mirror.
    /// `owner_alias`      – display name of the portfolio owner
    /// `strategy_desc`    – short strategy description
    /// `asset_allocation` – encoded allocation e.g. "BTC:40,ETH:35,XLM:25"
    /// Returns the new portfolio_id.
    pub fn register_portfolio(
        env: Env,
        owner_alias: String,
        strategy_desc: String,
        asset_allocation: String,
    ) -> u64 {
        let mut count: u64 = env.storage().instance().get(&COUNT_PORTFOLIO).unwrap_or(0);
        count += 1;

        let now = env.ledger().timestamp();

        let portfolio = Portfolio {
            portfolio_id: count,
            owner_alias: owner_alias.clone(),
            strategy_desc,
            asset_allocation,
            created_at: now,
            last_updated: now,
            mirror_count: 0,
            is_active: true,
        };

        let mut stats = Self::view_stats(env.clone());
        stats.total_portfolios += 1;

        env.storage().instance().set(&Portfoliobook::Portfolio(count), &portfolio);
        env.storage().instance().set(&ALL_STATS, &stats);
        env.storage().instance().set(&COUNT_PORTFOLIO, &count);
        env.storage().instance().extend_ttl(5000, 5000);

        log!(&env, "Portfolio registered: id={}, owner={}", count, owner_alias);
        count
    }

    /// Follower subscribes to mirror a master portfolio.
    /// `portfolio_id`    – the master portfolio to follow
    /// `follower_alias`  – display name of the follower
    /// Returns the new mirror_id.
    pub fn mirror_portfolio(
        env: Env,
        portfolio_id: u64,
        follower_alias: String,
    ) -> u64 {
        let mut portfolio = Self::view_portfolio(env.clone(), portfolio_id);

        assert!(portfolio.is_active, "Portfolio is not open for mirroring");

        let mut mirror_count: u64 = env.storage().instance().get(&COUNT_MIRROR).unwrap_or(0);
        mirror_count += 1;

        let now = env.ledger().timestamp();

        // Snapshot current allocation at subscription time
        let mirror = MirrorEntry {
            mirror_id: mirror_count,
            portfolio_id,
            follower_alias: follower_alias.clone(),
            synced_allocation: portfolio.asset_allocation.clone(),
            subscribed_at: now,
            last_sync: now,
            sync_count: 1,
            is_active: true,
        };

        portfolio.mirror_count += 1;

        let mut stats = Self::view_stats(env.clone());
        stats.total_mirrors  += 1;
        stats.active_mirrors += 1;
        stats.total_syncs    += 1;

        env.storage().instance().set(&Mirrorbook::Mirror(mirror_count), &mirror);
        env.storage().instance().set(&Portfoliobook::Portfolio(portfolio_id), &portfolio);
        env.storage().instance().set(&ALL_STATS, &stats);
        env.storage().instance().set(&COUNT_MIRROR, &mirror_count);
        env.storage().instance().extend_ttl(5000, 5000);

        log!(&env, "Mirror created: mirror_id={}, portfolio_id={}, follower={}",
            mirror_count, portfolio_id, follower_alias);
        mirror_count
    }

    /// Owner updates the asset allocation of their master portfolio.
    /// All active mirrors should call `sync_mirror` after this to pull the new allocation.
    /// `portfolio_id`     – the portfolio to update
    /// `new_allocation`   – updated allocation string e.g. "BTC:50,ETH:30,XLM:20"
    pub fn update_allocation(
        env: Env,
        portfolio_id: u64,
        new_allocation: String,
    ) {
        let mut portfolio = Self::view_portfolio(env.clone(), portfolio_id);

        assert!(portfolio.is_active, "Portfolio is inactive and cannot be updated");

        let now = env.ledger().timestamp();
        portfolio.asset_allocation = new_allocation.clone();
        portfolio.last_updated     = now;

        env.storage().instance().set(&Portfoliobook::Portfolio(portfolio_id), &portfolio);
        env.storage().instance().extend_ttl(5000, 5000);

        log!(&env, "Portfolio {} allocation updated to: {}", portfolio_id, new_allocation);
    }

    /// Follower syncs their mirror to the latest master portfolio allocation.
    /// `mirror_id` – the mirror subscription to sync
    pub fn sync_mirror(env: Env, mirror_id: u64) {
        let mut mirror = Self::view_mirror(env.clone(), mirror_id);

        assert!(mirror.is_active, "Mirror subscription is inactive");

        let portfolio = Self::view_portfolio(env.clone(), mirror.portfolio_id);

        assert!(portfolio.is_active, "Master portfolio is no longer active");

        let now = env.ledger().timestamp();
        mirror.synced_allocation = portfolio.asset_allocation.clone();
        mirror.last_sync         = now;
        mirror.sync_count       += 1;

        let mut stats = Self::view_stats(env.clone());
        stats.total_syncs += 1;

        env.storage().instance().set(&Mirrorbook::Mirror(mirror_id), &mirror);
        env.storage().instance().set(&ALL_STATS, &stats);
        env.storage().instance().extend_ttl(5000, 5000);

        log!(&env, "Mirror {} synced to portfolio {}: allocation={}",
            mirror_id, mirror.portfolio_id, portfolio.asset_allocation);
    }

    // ── View Helpers ──────────────────────────────────────────────────────────

    /// Returns the full Portfolio struct for a given portfolio_id.
    pub fn view_portfolio(env: Env, portfolio_id: u64) -> Portfolio {
        env.storage().instance().get(&Portfoliobook::Portfolio(portfolio_id))
            .unwrap_or_else(|| panic!("Portfolio not found: {}", portfolio_id))
    }

    /// Returns the full MirrorEntry struct for a given mirror_id.
    pub fn view_mirror(env: Env, mirror_id: u64) -> MirrorEntry {
        env.storage().instance().get(&Mirrorbook::Mirror(mirror_id))
            .unwrap_or_else(|| panic!("Mirror not found: {}", mirror_id))
    }

    /// Returns platform-wide mirroring statistics.
    pub fn view_stats(env: Env) -> MirrorStats {
        env.storage().instance().get(&ALL_STATS).unwrap_or(MirrorStats {
            total_portfolios: 0,
            total_mirrors:    0,
            active_mirrors:   0,
            total_syncs:      0,
        })
    }
}