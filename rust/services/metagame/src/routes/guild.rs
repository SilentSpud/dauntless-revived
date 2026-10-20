//! `routes/guild.ts`: the eleven v2 guild routes the 1.4.4 client uses. Rules and reply shapes are
//! in `guild.rs`. Every client route acts as the bearer token's own account; ids in the URL only
//! name the other player, the invite or the guild. The create comes from the Ramsgate game server
//! with its key.
//!
//! `GUILDS=0` puts the old stubs back: GET /guild answers 204, GET /guild/invite/player an empty
//! list, and every other guild route 404s as before.
//!
//! Order matters: DELETE /guild/player and /guild/player/:accountId come before DELETE
//! /guild/:guildId.

// Refusals are axum responses, the way the extractors report them; boxing them gains nothing here.
#![allow(clippy::result_large_err)]

use axum::extract::State;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::{Value, json};
use service_kit::body::ExpressBody;
use service_kit::routing::{Params, Router};

use super::{empty, fall_through, field, json};
use crate::app::{AppError, AppState};
use crate::auth::{GameServer, MetagameAuth, Player, metagame_auth};
use crate::guild::{self, Binding, Created, Reply};
use crate::time::now_ms;

pub fn routes() -> Router<AppState> {
    Router::new()
        .get("/guild/invite/player", invites)
        .get("/guild", own_guild)
        .post("/guild/validate", validate)
        .post("/guild", create)
        .delete("/guild/player", leave)
        .delete("/guild/player/:accountId", kick)
        .put("/guild/invite/:accountId", invite)
        .post("/guild/invite/accept/:inviteId", accept)
        .delete("/guild/invite/:inviteId", decline)
        .put("/guild/rank/:accountId/:rank", rank)
        .delete("/guild/:guildId", disband)
}

fn send(reply: Reply) -> Response {
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    match reply.body {
        None => empty(status),
        Some(body) => json(status, body),
    }
}

/// `Caller`: the token's account, when it names one.
fn caller(auth: &MetagameAuth) -> Option<String> {
    auth.user_id().filter(|id| !id.is_empty()).map(str::to_owned)
}

/// `GuildsOn` and `PlayerTokenOnly` behind the auth: the player, or the answer to send.
async fn player_on(state: &AppState, parts: &Parts) -> Result<Player, Response> {
    if !state.guilds.enabled {
        return Err(fall_through(parts));
    }
    let auth = metagame_auth(parts, state).await?;
    match auth.user_id().filter(|id| !id.is_empty()) {
        Some(user_id) => Ok(Player {
            user_id: user_id.to_owned(),
            via_game_server: auth.is_game_server(),
        }),
        None => {
            tracing::warn!(
                "Refusing {} {}: it needs a player's token",
                parts.method,
                parts.uri.path()
            );
            Err(empty(StatusCode::FORBIDDEN))
        }
    }
}

/// `Sweep`: at most once a minute, expired invites go.
async fn sweep(state: &AppState) -> Result<(), AppError> {
    let now = now_ms();
    if guild::sweep_due(&state.guild_memory, now) {
        state
            .store
            .write(move |conn| guild::delete_expired_invites(conn, now).map_err(AppError::from))
            .await?;
    }
    Ok(())
}

/// The caller's open invites, read at login, at each world load and after every guild action.
async fn invites(State(state): State<AppState>, auth: MetagameAuth) -> Result<Response, AppError> {
    if !state.guilds.enabled {
        tracing::info!("Guild invites (stubbed)");
        return Ok(json(
            StatusCode::OK,
            json!({"code": null, "message": "OK", "payload": {"invites": []}}),
        ));
    }
    sweep(&state).await?;
    let user = caller(&auth);
    let now = now_ms();
    let reply = state
        .store
        .read(move |conn| guild::own_invites(conn, user.as_deref(), now).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}

/// The caller's guild; 204 when there is none (a failure to the client, which then clears its
/// guild).
async fn own_guild(State(state): State<AppState>, auth: MetagameAuth) -> Result<Response, AppError> {
    if !state.guilds.enabled {
        tracing::info!("Current guild (stubbed)");
        return Ok(empty(StatusCode::NO_CONTENT));
    }
    let user = caller(&auth);
    let shared = state.clone();
    let reply = state
        .store
        .read(move |conn| guild::own_guild(conn, &shared.guilds, user.as_deref()).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}

/// `{leader_account_id, name, nameplate}`, sent while the player types in CREATE A GUILD. The
/// token's account decides; a different leader in the body is only logged.
async fn validate(
    State(state): State<AppState>,
    parts: Parts,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    let player = match player_on(&state, &parts).await {
        Ok(player) => player,
        Err(refused) => return Ok(refused),
    };
    sweep(&state).await?;
    let user = player.user_id;
    if let Some(Value::String(leader)) = field(&body, "leader_account_id")
        && !leader.is_empty()
        && *leader != user
    {
        tracing::info!(
            "guild: validate by {user} names leader {}; checking the caller",
            if crate::accounts::is_account_id_shape(leader) {
                leader.as_str()
            } else {
                "<not an account id>"
            }
        );
    }
    let now = now_ms();
    let name = field(&body, "name").cloned();
    let nameplate = field(&body, "nameplate").cloned();
    let (shared, id, n, p) = (state.clone(), user.clone(), name.clone(), nameplate.clone());
    let checked = state
        .store
        .read(move |conn| {
            guild::validate_in_tx(conn, &shared.guilds, &id, n.as_ref(), p.as_ref()).map_err(AppError::from)
        })
        .await?;
    Ok(send(guild::validate_finish(
        &state.guild_memory,
        &user,
        name.as_ref(),
        nameplate.as_ref(),
        checked,
        now,
    )))
}

/// The game server's create: the same body and the game-server key. A token that comes along is the
/// game server's own login's, never the leader's, so it is only logged. Refusals that are server
/// policy answer code "" (the client shows "Unable to create guild."); the reason is in the log.
async fn create(
    State(state): State<AppState>,
    parts: Parts,
    ExpressBody(body): ExpressBody,
) -> Result<Response, AppError> {
    if !state.guilds.enabled {
        return Ok(fall_through(&parts));
    }
    let server =
        match <GameServer as axum::extract::FromRequestParts<AppState>>::from_request_parts(&mut parts.clone(), &state)
            .await
        {
            Ok(server) => server,
            Err(refused) => return Ok(refused),
        };
    sweep(&state).await?;
    let now = now_ms();
    let leader_value = field(&body, "leader_account_id").cloned();
    let name = field(&body, "name").cloned();
    let nameplate = field(&body, "nameplate").cloned();
    let leader = leader_value
        .as_ref()
        .and_then(Value::as_str)
        .filter(|l| crate::accounts::is_account_id_shape(l))
        .map(str::to_owned);
    let shown = leader.as_deref().unwrap_or("<not an account id>").to_owned();
    if let Some(forwarded) = &server.forwarded_user_id
        && leader_value.as_ref().and_then(Value::as_str) != Some(forwarded.as_str())
    {
        tracing::info!(
            "guild: create for {shown}: the game server's token names {forwarded} (its own login, not the leader's); not used"
        );
    }
    let exists = match &leader {
        None => false,
        Some(leader) => {
            let id = leader.clone();
            state
                .store
                .read(move |conn| guild::leader_exists(conn, &id).map_err(AppError::from))
                .await?
        }
    };
    let Some(leader) = leader.clone().filter(|_| exists) else {
        let reply = guild::no_such_account();
        guild::create_refusal_log(
            leader.as_deref(),
            name.as_ref(),
            nameplate.as_ref(),
            &reply,
            "no such account",
        );
        return Ok(send(reply));
    };
    let seen = state
        .parties
        .lock()
        .seen_within(&leader, guild::CREATE_ACTIVITY_MS, now);
    let (binding, others) = guild::create_binding(
        &state.guild_memory,
        &state.guilds,
        &leader,
        name.as_ref(),
        nameplate.as_ref(),
        seen,
        now,
    );
    let Some(binding) = binding else {
        let reply = guild::unbound_create(&state.guilds, name.as_ref(), nameplate.as_ref());
        let why = format!(
            "no validate of this name and nameplate by the leader in the last 15 minutes{}",
            if others > 0 {
                format!(" ({others} other validated name(s))")
            } else {
                String::new()
            }
        );
        guild::create_refusal_log(Some(&leader), name.as_ref(), nameplate.as_ref(), &reply, &why);
        return Ok(send(reply));
    };
    if binding == Binding::Fallback {
        let shown = guild::quoted(name.as_ref());
        tracing::warn!(
            "guild: create for {leader} name={shown} accepted without a validate of this name (GUILD_CREATE_ACTIVITY_FALLBACK=1)"
        );
    }
    let (shared, id, n, p) = (state.clone(), leader.clone(), name.clone(), nameplate.clone());
    let created = state
        .store
        .write(move |conn| {
            guild::create_in_tx(
                conn,
                &shared.guilds,
                &shared.guild_memory,
                &id,
                n.as_ref(),
                p.as_ref(),
                now,
            )
            .map_err(AppError::from)
        })
        .await?;
    let data = match created {
        Created::Refused(reply) => {
            guild::create_refusal_log(Some(&leader), name.as_ref(), nameplate.as_ref(), &reply, "checks");
            return Ok(send(reply));
        }
        Created::Done(data) => data,
    };
    guild::created(&state.guild_memory, &leader, now);
    tracing::info!(
        "guild: created G={} name={} tag={} leader={leader} ({}{})",
        crate::js::string(data.get("id")),
        crate::js::string(data.get("name")),
        crate::js::string(data.get("nameplate")),
        if binding == Binding::Ticket {
            "validated name"
        } else {
            "activity fallback"
        },
        match &server.forwarded_user_id {
            Some(forwarded) => format!(", a token of {forwarded} came along"),
            None => ", no token".to_owned(),
        }
    );
    Ok(send(guild::created_reply(data)))
}

/// Leave Guild (no body).
async fn leave(State(state): State<AppState>, parts: Parts) -> Result<Response, AppError> {
    let player = match player_on(&state, &parts).await {
        Ok(player) => player,
        Err(refused) => return Ok(refused),
    };
    let reply = state
        .store
        .write(move |conn| guild::leave(conn, &player.user_id).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}

/// Kick From Guild.
async fn kick(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let player = match player_on(&state, &parts).await {
        Ok(player) => player,
        Err(refused) => return Ok(refused),
    };
    let target = params.get("accountId").unwrap_or("").to_owned();
    let reply = state
        .store
        .write(move |conn| guild::kick(conn, &player.user_id, &target).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}

/// Invite to Guild (no body): a Leader or Officer invites a player to their guild.
async fn invite(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let player = match player_on(&state, &parts).await {
        Ok(player) => player,
        Err(refused) => return Ok(refused),
    };
    sweep(&state).await?;
    let now = now_ms();
    let target = params.get("accountId").unwrap_or("").to_owned();
    let (shared, user, them) = (state.clone(), player.user_id.clone(), target.clone());
    let (reply, why) = state
        .store
        .write(move |conn| {
            guild::invite_in_tx(conn, &shared.guilds, &shared.guild_memory, &user, &them, now).map_err(AppError::from)
        })
        .await?;
    guild::invite_finish(&state.guild_memory, &player.user_id, &target, &reply, why, now);
    Ok(send(reply))
}

/// Accept Guild Invite; the URL carries the invite's id.
async fn accept(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let player = match player_on(&state, &parts).await {
        Ok(player) => player,
        Err(refused) => return Ok(refused),
    };
    let now = now_ms();
    let invite = params.get("inviteId").unwrap_or("").to_owned();
    let shared = state.clone();
    let reply = state
        .store
        .write(move |conn| guild::accept(conn, &shared.guilds, &player.user_id, &invite, now).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}

/// Decline Guild Invite.
async fn decline(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let player = match player_on(&state, &parts).await {
        Ok(player) => player,
        Err(refused) => return Ok(refused),
    };
    let now = now_ms();
    let invite = params.get("inviteId").unwrap_or("").to_owned();
    let shared = state.clone();
    let reply = state
        .store
        .write(move |conn| {
            guild::decline(conn, &shared.guild_memory, &player.user_id, &invite, now).map_err(AppError::from)
        })
        .await?;
    Ok(send(reply))
}

/// Promote To Guild Officer (officer), Demote To Guild Member (member), Promote To Guild Leader
/// (leader).
async fn rank(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let player = match player_on(&state, &parts).await {
        Ok(player) => player,
        Err(refused) => return Ok(refused),
    };
    let now = now_ms();
    let target = params.get("accountId").unwrap_or("").to_owned();
    let rank = params.get("rank").unwrap_or("").to_owned();
    let reply = state
        .store
        .write(move |conn| guild::change_rank(conn, &player.user_id, &target, &rank, now).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}

/// DISBAND GUILD (leader only). Last: it would otherwise take DELETE /guild/player.
async fn disband(State(state): State<AppState>, parts: Parts, params: Params) -> Result<Response, AppError> {
    let player = match player_on(&state, &parts).await {
        Ok(player) => player,
        Err(refused) => return Ok(refused),
    };
    let guild = params.get("guildId").unwrap_or("").to_owned();
    let reply = state
        .store
        .write(move |conn| guild::disband(conn, &player.user_id, &guild).map_err(AppError::from))
        .await?;
    Ok(send(reply))
}
