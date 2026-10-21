//! The Discord key bot (`legacy/UndauntedDiscordBot`, MIGRATION_PLAN.md S5): `/key claim` hands a
//! Discord user one launcher invite by DM, `/key link` ties a Discord account to an existing game
//! account by its launcher key, `/key recover` rotates a lost key through the metagame, and the
//! owner's `/counter` keeps a live member count.

pub mod bot;
pub mod commands;
pub mod counter;
pub mod discord;
pub mod gateway;
pub mod instance;
pub mod invite;
pub mod keys;
pub mod link_input;
pub mod metagame;
pub mod recovery;
