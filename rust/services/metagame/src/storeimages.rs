//! Store offer pictures (`routes/store.ts`, MIGRATION_PLAN.md M12): `store-images/` next to the
//! binary holds one PNG per catalog id (lower case), extracted from the 1.4.4 client. The folder is
//! listed once; an offer whose first item has a picture gets its URL on this server.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use axum::extract::Request;
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, HeaderValue};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt;
use tower_http::services::ServeFile;

pub struct StoreImages {
    dir: PathBuf,
    files: HashSet<String>,
}

impl StoreImages {
    /// Lists `dir`; a missing folder means no pictures.
    pub fn load(dir: &Path) -> Self {
        let files = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            dir: dir.to_owned(),
            files,
        }
    }

    /// The picture file of a catalog id, if there is one (`StoreImageFor`).
    pub fn file_for(&self, catalog_id: &str) -> Option<String> {
        let file = format!("{}.png", catalog_id.to_lowercase());
        self.files.contains(&file).then_some(file)
    }

    /// `GET /store-images/:file`: only `[a-z0-9_]+.png` names that are in the folder; cached a day.
    pub async fn serve(&self, file: &str, request: Request) -> Response {
        let valid = file.strip_suffix(".png").is_some_and(|stem| {
            !stem.is_empty()
                && stem
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        });
        if !valid || !self.files.contains(file) {
            return StatusCode::NOT_FOUND.into_response();
        }
        let mut response = match ServeFile::new(self.dir.join(file)).oneshot(request).await {
            Ok(response) => response.map(axum::body::Body::new),
            Err(never) => match never {},
        };
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("public, max-age=86400"));
        response
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::header::CONTENT_TYPE;

    use super::*;

    #[tokio::test]
    async fn serves_listed_pictures_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ac_head_trials_00.png"), b"\x89PNG fake").unwrap();
        std::fs::write(dir.path().join(".hidden.png"), b"x").unwrap();
        let images = StoreImages::load(dir.path());
        assert_eq!(
            images.file_for("AC_Head_Trials_00").as_deref(),
            Some("ac_head_trials_00.png")
        );
        assert_eq!(images.file_for("missing"), None);

        let get = |file: &'static str| {
            let images = &images;
            async move {
                images
                    .serve(
                        file,
                        Request::get(format!("/store-images/{file}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
            }
        };
        let ok = get("ac_head_trials_00.png").await;
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(ok.headers()[CONTENT_TYPE], "image/png");
        assert_eq!(ok.headers()[CACHE_CONTROL], "public, max-age=86400");
        assert_eq!(
            get(".hidden.png").await.status(),
            StatusCode::NOT_FOUND,
            "dotfiles never"
        );
        assert_eq!(get("AC_HEAD_TRIALS_00.png").await.status(), StatusCode::NOT_FOUND);
        assert_eq!(get("missing.png").await.status(), StatusCode::NOT_FOUND);
    }
}
