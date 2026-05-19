use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tokio::sync::RwLock;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PodcastData {
    pub title: String,
    pub description: String,
    pub website: String,
    pub feed_slug: String,
    pub cover: Option<String>,
    pub categories: Vec<String>,
    pub primary_category: String,
    pub language: String,
}

impl Default for PodcastData {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            website: String::new(),
            feed_slug: String::new(),
            cover: None,
            categories: Vec::new(),
            primary_category: String::new(),
            language: "en".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub id: String,
    pub audio_file_name: Option<String>,
    pub audio_size: u64,
    pub title: String,
    pub notes: String,
    #[serde(rename = "type")]
    pub episode_type: EpisodeType,
    pub number: u32,
    pub cover: Option<String>,
}

impl Episode {
    pub fn empty(number: u32) -> Self {
        Self {
            id: new_id(),
            audio_file_name: None,
            audio_size: 0,
            title: String::new(),
            notes: String::new(),
            episode_type: EpisodeType::Full,
            number,
            cover: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EpisodeType {
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PodcastEntry {
    pub id: String,
    pub data: PodcastData,
    pub episodes: Vec<Episode>,
}

impl PodcastEntry {
    pub fn new(data: PodcastData) -> Self {
        Self {
            id: new_id(),
            data,
            episodes: vec![Episode::empty(1)],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppDocument {
    pub revision: u64,
    pub profile: Profile,
    pub podcasts: Vec<PodcastEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreatePodcastRequest {
    #[serde(default)]
    pub data: PodcastData,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreateEpisodeRequest {
    #[serde(default)]
    pub episode: Option<Episode>,
}

#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
    inner: Arc<RwLock<AppDocument>>,
}

impl Store {
    pub async fn load(path: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let path = path.into();
        let document = match tokio::fs::read_to_string(&path).await {
            Ok(raw) => serde_json::from_str(&raw)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppDocument::default(),
            Err(error) => return Err(error.into()),
        };

        Ok(Self {
            path,
            inner: Arc::new(RwLock::new(document)),
        })
    }

    pub async fn state(&self) -> AppDocument {
        self.inner.read().await.clone()
    }

    pub async fn save_profile(&self, profile: Profile) -> Result<AppDocument, StoreError> {
        let snapshot = {
            let mut document = self.inner.write().await;
            document.profile = profile;
            document.revision += 1;
            document.clone()
        };
        self.persist(&snapshot).await?;
        Ok(snapshot)
    }

    pub async fn create_podcast(&self, data: PodcastData) -> Result<PodcastEntry, StoreError> {
        let podcast = PodcastEntry::new(data);
        let snapshot = {
            let mut document = self.inner.write().await;
            document.podcasts.push(podcast.clone());
            document.revision += 1;
            document.clone()
        };
        self.persist(&snapshot).await?;
        Ok(podcast)
    }

    pub async fn save_podcast(
        &self,
        podcast_id: &str,
        data: PodcastData,
    ) -> Result<PodcastEntry, StoreError> {
        let (podcast, snapshot) = {
            let mut document = self.inner.write().await;
            let podcast = find_podcast_mut(&mut document, podcast_id)?;
            podcast.data = data;
            let podcast = podcast.clone();
            document.revision += 1;
            (podcast, document.clone())
        };
        self.persist(&snapshot).await?;
        Ok(podcast)
    }

    pub async fn delete_podcast(&self, podcast_id: &str) -> Result<AppDocument, StoreError> {
        let snapshot = {
            let mut document = self.inner.write().await;
            let original_len = document.podcasts.len();
            document.podcasts.retain(|podcast| podcast.id != podcast_id);
            if document.podcasts.len() == original_len {
                return Err(StoreError::NotFound("podcast"));
            }
            document.revision += 1;
            document.clone()
        };
        self.persist(&snapshot).await?;
        Ok(snapshot)
    }

    pub async fn create_episode(
        &self,
        podcast_id: &str,
        episode: Option<Episode>,
    ) -> Result<Episode, StoreError> {
        let (episode, snapshot) = {
            let mut document = self.inner.write().await;
            let podcast = find_podcast_mut(&mut document, podcast_id)?;
            let mut episode =
                episode.unwrap_or_else(|| Episode::empty(next_episode_number(podcast)));
            if episode.id.trim().is_empty() {
                episode.id = new_id();
            }
            if episode.number == 0 {
                episode.number = next_episode_number(podcast);
            }
            podcast.episodes.push(episode.clone());
            renumber_episodes(&mut podcast.episodes);
            let episode = podcast
                .episodes
                .iter()
                .find(|item| item.id == episode.id)
                .cloned()
                .expect("newly inserted episode should exist");
            document.revision += 1;
            (episode, document.clone())
        };
        self.persist(&snapshot).await?;
        Ok(episode)
    }

    pub async fn save_episode(
        &self,
        podcast_id: &str,
        episode_id: &str,
        episode: Episode,
    ) -> Result<Episode, StoreError> {
        let (episode, snapshot) = {
            let mut document = self.inner.write().await;
            let podcast = find_podcast_mut(&mut document, podcast_id)?;
            let current = podcast
                .episodes
                .iter_mut()
                .find(|item| item.id == episode_id)
                .ok_or(StoreError::NotFound("episode"))?;
            *current = Episode {
                id: episode_id.to_string(),
                ..episode
            };
            renumber_episodes(&mut podcast.episodes);
            let episode = podcast
                .episodes
                .iter()
                .find(|item| item.id == episode_id)
                .cloned()
                .expect("updated episode should exist");
            document.revision += 1;
            (episode, document.clone())
        };
        self.persist(&snapshot).await?;
        Ok(episode)
    }

    pub async fn delete_episode(
        &self,
        podcast_id: &str,
        episode_id: &str,
    ) -> Result<PodcastEntry, StoreError> {
        let (podcast, snapshot) = {
            let mut document = self.inner.write().await;
            let podcast = find_podcast_mut(&mut document, podcast_id)?;
            let original_len = podcast.episodes.len();
            podcast.episodes.retain(|episode| episode.id != episode_id);
            if podcast.episodes.len() == original_len {
                return Err(StoreError::NotFound("episode"));
            }
            renumber_episodes(&mut podcast.episodes);
            let podcast = podcast.clone();
            document.revision += 1;
            (podcast, document.clone())
        };
        self.persist(&snapshot).await?;
        Ok(podcast)
    }

    async fn persist(&self, document: &AppDocument) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let temp_path = self.path.with_extension("json.tmp");
        let data = serde_json::to_vec_pretty(document)?;
        tokio::fs::write(&temp_path, data).await?;
        tokio::fs::rename(&temp_path, &self.path).await?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody {
    error: String,
}

impl IntoResponse for StoreError {
    fn into_response(self) -> Response {
        let status = match self {
            StoreError::NotFound(_) => StatusCode::NOT_FOUND,
            StoreError::Io(_) | StoreError::Json(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(ErrorBody {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

pub fn build_router(store: Store) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/state", get(get_state))
        .route("/api/profile", put(put_profile))
        .route("/api/podcasts", get(list_podcasts).post(post_podcast))
        .route(
            "/api/podcasts/:podcast_id",
            get(get_podcast).put(put_podcast).delete(delete_podcast),
        )
        .route("/api/podcasts/:podcast_id/episodes", post(post_episode))
        .route(
            "/api/podcasts/:podcast_id/episodes/:episode_id",
            put(put_episode).delete(delete_episode),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(store)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true }))
}

async fn get_state(State(store): State<Store>) -> Json<AppDocument> {
    Json(store.state().await)
}

async fn put_profile(
    State(store): State<Store>,
    Json(profile): Json<Profile>,
) -> Result<Json<AppDocument>, StoreError> {
    store.save_profile(profile).await.map(Json)
}

async fn list_podcasts(State(store): State<Store>) -> Json<Vec<PodcastEntry>> {
    Json(store.state().await.podcasts)
}

async fn get_podcast(
    State(store): State<Store>,
    Path(podcast_id): Path<String>,
) -> Result<Json<PodcastEntry>, StoreError> {
    store
        .state()
        .await
        .podcasts
        .into_iter()
        .find(|podcast| podcast.id == podcast_id)
        .map(Json)
        .ok_or(StoreError::NotFound("podcast"))
}

async fn post_podcast(
    State(store): State<Store>,
    Json(payload): Json<CreatePodcastRequest>,
) -> Result<(StatusCode, Json<PodcastEntry>), StoreError> {
    let podcast = store.create_podcast(payload.data).await?;
    Ok((StatusCode::CREATED, Json(podcast)))
}

async fn put_podcast(
    State(store): State<Store>,
    Path(podcast_id): Path<String>,
    Json(data): Json<PodcastData>,
) -> Result<Json<PodcastEntry>, StoreError> {
    store.save_podcast(&podcast_id, data).await.map(Json)
}

async fn delete_podcast(
    State(store): State<Store>,
    Path(podcast_id): Path<String>,
) -> Result<Json<AppDocument>, StoreError> {
    store.delete_podcast(&podcast_id).await.map(Json)
}

async fn post_episode(
    State(store): State<Store>,
    Path(podcast_id): Path<String>,
    Json(payload): Json<CreateEpisodeRequest>,
) -> Result<(StatusCode, Json<Episode>), StoreError> {
    let episode = store.create_episode(&podcast_id, payload.episode).await?;
    Ok((StatusCode::CREATED, Json(episode)))
}

async fn put_episode(
    State(store): State<Store>,
    Path((podcast_id, episode_id)): Path<(String, String)>,
    Json(episode): Json<Episode>,
) -> Result<Json<Episode>, StoreError> {
    store
        .save_episode(&podcast_id, &episode_id, episode)
        .await
        .map(Json)
}

async fn delete_episode(
    State(store): State<Store>,
    Path((podcast_id, episode_id)): Path<(String, String)>,
) -> Result<Json<PodcastEntry>, StoreError> {
    store
        .delete_episode(&podcast_id, &episode_id)
        .await
        .map(Json)
}

fn find_podcast_mut<'a>(
    document: &'a mut AppDocument,
    podcast_id: &str,
) -> Result<&'a mut PodcastEntry, StoreError> {
    document
        .podcasts
        .iter_mut()
        .find(|podcast| podcast.id == podcast_id)
        .ok_or(StoreError::NotFound("podcast"))
}

fn next_episode_number(podcast: &PodcastEntry) -> u32 {
    podcast.episodes.len() as u32 + 1
}

fn renumber_episodes(episodes: &mut [Episode]) {
    episodes.sort_by_key(|episode| episode.number);
    for (index, episode) in episodes.iter_mut().enumerate() {
        episode.number = index as u32 + 1;
    }
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    fn podcast_data(title: &str) -> PodcastData {
        PodcastData {
            title: title.to_string(),
            description: "A good show".to_string(),
            website: "https://example.com".to_string(),
            feed_slug: "good-show".to_string(),
            cover: None,
            categories: vec!["Arts".to_string()],
            primary_category: "Arts".to_string(),
            language: "en".to_string(),
        }
    }

    async fn read_json<T: for<'de> Deserialize<'de>>(body: Body) -> T {
        let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn store_persists_profile_podcast_and_episode_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let store = Store::load(&path).await.unwrap();

        let state = store
            .save_profile(Profile {
                name: "Q".to_string(),
                email: "q@example.com".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(state.revision, 1);

        let podcast = store.create_podcast(podcast_data("First")).await.unwrap();
        let saved = store
            .save_podcast(&podcast.id, podcast_data("Updated"))
            .await
            .unwrap();
        assert_eq!(saved.data.title, "Updated");

        let episode = store.create_episode(&podcast.id, None).await.unwrap();
        let episode_id = episode.id.clone();
        let updated_episode = store
            .save_episode(
                &podcast.id,
                &episode_id,
                Episode {
                    title: "Episode One".to_string(),
                    ..episode
                },
            )
            .await
            .unwrap();
        assert_eq!(updated_episode.title, "Episode One");

        let loaded = Store::load(&path).await.unwrap().state().await;
        assert_eq!(loaded.profile.email, "q@example.com");
        assert_eq!(loaded.podcasts[0].data.title, "Updated");
        assert_eq!(loaded.podcasts[0].episodes[1].title, "Episode One");
    }

    #[tokio::test]
    async fn deleting_episode_renumbers_remaining_episodes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load(dir.path().join("state.json")).await.unwrap();
        let podcast = store.create_podcast(podcast_data("Show")).await.unwrap();
        let episode = store.create_episode(&podcast.id, None).await.unwrap();

        let podcast = store
            .delete_episode(&podcast.id, &episode.id)
            .await
            .unwrap();

        assert_eq!(podcast.episodes.len(), 1);
        assert_eq!(podcast.episodes[0].number, 1);
    }

    #[tokio::test]
    async fn router_exposes_crud_endpoints() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load(dir.path().join("state.json")).await.unwrap();
        let app = build_router(store);

        let request = Request::builder()
            .method("POST")
            .uri("/api/podcasts")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&CreatePodcastRequest {
                    data: podcast_data("API Show"),
                })
                .unwrap(),
            ))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let podcast: PodcastEntry = read_json(response.into_body()).await;

        let mut updated = podcast.data.clone();
        updated.title = "Saved API Show".to_string();
        let request = Request::builder()
            .method("PUT")
            .uri(format!("/api/podcasts/{}", podcast.id))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&updated).unwrap()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let podcast: PodcastEntry = read_json(response.into_body()).await;
        assert_eq!(podcast.data.title, "Saved API Show");

        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/api/podcasts/{}", podcast.id))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let state: AppDocument = read_json(response.into_body()).await;
        assert!(state.podcasts.is_empty());
    }
}
