//! `/key resend` and `/key recover` (`recovery.mjs`). Both answer only privately: a public
//! interaction is refused before anything is looked up, and a key goes by DM, or into the private
//! reply when DMs are closed.

use serde_json::{Value, json};

use crate::keys::{BoxFuture, Failure, Outcome};

/// One `/key` interaction, as the flows need it.
pub trait Interaction: Send + Sync {
    /// The acknowledgement is ephemeral
    fn ephemeral(&self) -> bool;
    fn user_id(&self) -> String;
    fn option(&self, name: &str) -> Option<String>;
    fn edit_reply(&self, content: String) -> BoxFuture<'_, Result<(), Failure>>;
    /// A DM to the actor; the message id
    fn send_dm(&self, content: String) -> BoxFuture<'_, Result<Option<String>, Failure>>;
}

fn private(interaction: &dyn Interaction) -> Result<(), Failure> {
    if interaction.ephemeral() {
        Ok(())
    } else {
        Err(Failure::new("Private acknowledgement required"))
    }
}

/// By DM, or in the private reply when the DM fails; never in public.
pub async fn send_privately(interaction: &dyn Interaction, content: String) -> Result<&'static str, Failure> {
    private(interaction)?;
    if interaction.send_dm(content.clone()).await.is_err() {
        interaction.edit_reply(content).await?;
        return Ok("ephemeral");
    }
    interaction
        .edit_reply("Your copy is in your DMs. Keep it private.".into())
        .await?;
    Ok("dm")
}

/// A read-only lookup: never issues another invite or resurrects a redeemed one.
pub async fn resend<F>(
    interaction: &dyn Interaction,
    status: F,
    format: impl Fn(&str) -> Result<String, Failure>,
    recovery: bool,
) -> Result<(), Failure>
where
    F: std::future::Future<Output = Result<Outcome, Failure>>,
{
    private(interaction)?;
    let result = status.await?;
    if let (Some(code), "ready") = (&result.code, result.status.as_str()) {
        send_privately(interaction, format(code)?).await?;
        return Ok(());
    }
    interaction
        .edit_reply(if recovery {
            "An unused saved invite could not be resent. If you already registered and verified /key link, use /key recover for a replacement account key. Otherwise contact support."
        } else {
            "An unused saved invite could not be resent. If you already registered, contact support to recover your existing account key. Account-key recovery is not enabled yet; no new invite was issued."
        }
        .into())
        .await
}

pub type Request<'a> = Box<dyn Fn(&'static str, Value) -> BoxFuture<'a, Result<Value, Failure>> + Send + Sync + 'a>;

/// `/key recover`: a challenge first, then with `confirm:` the replacement key.
pub async fn recover(interaction: &dyn Interaction, request: Request<'_>) -> Result<(), Failure> {
    private(interaction)?;
    let Some(challenge) = interaction.option("confirm") else {
        let result = request("begin", json!({})).await?;
        let reply = match result["challenge"].as_str().filter(|c| !c.is_empty()) {
            Some(challenge) => format!(
                "Recovery requires an account link previously verified with your launcher key using /key link. No username or moderator role is sufficient. Confirm within five minutes with:\n/key recover confirm:{challenge}\nThis revokes your old key. If you never verified a link and lost the key, contact support."
            ),
            None => "Recovery is unavailable or on cooldown. Try later or contact support.".into(),
        };
        return interaction.edit_reply(reply).await;
    };
    // The private interaction must still be writable before the key rotates
    interaction
        .edit_reply("Verifying your recovery request…".into())
        .await?;
    let result = request("confirm", json!({ "challenge": challenge })).await?;
    let Some(key) = result["key"].as_str().filter(|k| !k.is_empty()) else {
        return interaction
            .edit_reply("Recovery could not be verified or is on cooldown. Use /key link with your existing key, or contact support.".into())
            .await;
    };
    let message = format!(
        "🔑 **Replacement Account Key**\n```\n{key}\n```\nYour old key is revoked. Save this key and sign in again."
    );
    if send_privately(interaction, message).await.is_err() {
        interaction
            .edit_reply("Your old key was revoked, but Discord did not confirm delivery. Check your DMs; if missing, repeat recovery after the cooldown.".into())
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct Fake {
        ephemeral: bool,
        blocked: bool,
        confirm: Option<String>,
        dms: Arc<Mutex<Vec<String>>>,
        replies: Arc<Mutex<Vec<String>>>,
    }

    impl Interaction for Fake {
        fn ephemeral(&self) -> bool {
            self.ephemeral
        }

        fn user_id(&self) -> String {
            "123456789012345678".into()
        }

        fn option(&self, _name: &str) -> Option<String> {
            self.confirm.clone()
        }

        fn edit_reply(&self, content: String) -> BoxFuture<'_, Result<(), Failure>> {
            self.replies.lock().unwrap().push(content);
            Box::pin(async { Ok(()) })
        }

        fn send_dm(&self, content: String) -> BoxFuture<'_, Result<Option<String>, Failure>> {
            let blocked = self.blocked;
            if !blocked {
                self.dms.lock().unwrap().push(content);
            }
            Box::pin(async move {
                if blocked {
                    Err(Failure::new("blocked"))
                } else {
                    Ok(None)
                }
            })
        }
    }

    fn fake(ephemeral: bool, blocked: bool) -> Fake {
        Fake {
            ephemeral,
            blocked,
            confirm: Some("a".repeat(64)),
            dms: Arc::default(),
            replies: Arc::default(),
        }
    }

    #[tokio::test]
    async fn recovery_delivers_privately_with_dms_open_or_blocked() {
        let key = format!("UUK_{}", "b".repeat(64));
        for blocked in [false, true] {
            let interaction = fake(true, blocked);
            let answer = json!({ "key": key });
            recover(
                &interaction,
                Box::new(move |_, _| {
                    let answer = answer.clone();
                    Box::pin(async move { Ok(answer) })
                }),
            )
            .await
            .unwrap();
            let (dms, replies) = (
                interaction.dms.lock().unwrap().clone(),
                interaction.replies.lock().unwrap().clone(),
            );
            if blocked {
                assert!(dms.is_empty());
                assert!(replies.last().unwrap().contains(&key));
            } else {
                assert!(dms[0].contains(&key));
                assert!(!replies.iter().any(|r| r.contains(&key)));
            }
        }
    }

    #[tokio::test]
    async fn a_public_interaction_cannot_rotate_or_disclose() {
        let interaction = fake(false, false);
        let error = recover(
            &interaction,
            Box::new(|_, _| Box::pin(async { panic!("must not rotate") })),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("Private acknowledgement"));
        assert!(send_privately(&interaction, "private".into()).await.is_err());
        assert!(interaction.dms.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn resend_reuses_the_actors_invite_and_never_a_redeemed_one() {
        let interaction = fake(true, false);
        let ready = async {
            Ok(Outcome {
                status: "ready".into(),
                code: Some("existing".into()),
            })
        };
        resend(&interaction, ready, |code| Ok(format!("invite:{code}")), false)
            .await
            .unwrap();
        assert_eq!(interaction.dms.lock().unwrap()[0], "invite:existing");
        let interaction = fake(true, false);
        resend(
            &interaction,
            async { Ok(Outcome::of("redeemed")) },
            |_| panic!("must not format"),
            false,
        )
        .await
        .unwrap();
        assert!(interaction.dms.lock().unwrap().is_empty());
        assert!(interaction.replies.lock().unwrap()[0].contains("contact support"));
    }
}
