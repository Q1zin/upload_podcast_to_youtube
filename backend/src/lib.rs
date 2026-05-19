use std::{path::PathBuf, sync::Arc};

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tokio::{io::AsyncWriteExt, sync::RwLock};
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};
use uuid::Uuid;

const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_AUDIO_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const REQUEST_BODY_LIMIT: usize = (MAX_AUDIO_BYTES as usize) + (64 * 1024 * 1024);
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];
const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "mp3", "aac", "aiff", "mp4", "m4a", "flac", "ogg", "mkv",
];

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
    resources_path: PathBuf,
    public_base_url: String,
    inner: Arc<RwLock<AppDocument>>,
}

impl Store {
    pub async fn load(path: impl Into<PathBuf>) -> Result<Self, StoreError> {
        Self::load_with_resources(path, "resurs", "http://127.0.0.1:8787").await
    }

    pub async fn load_with_resources(
        path: impl Into<PathBuf>,
        resources_path: impl Into<PathBuf>,
        public_base_url: impl Into<String>,
    ) -> Result<Self, StoreError> {
        let path = path.into();
        let resources_path = resources_path.into();
        let public_base_url = public_base_url.into().trim_end_matches('/').to_string();
        let document = match tokio::fs::read_to_string(&path).await {
            Ok(raw) => serde_json::from_str(&raw)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppDocument::default(),
            Err(error) => return Err(error.into()),
        };

        Ok(Self {
            path,
            resources_path,
            public_base_url,
            inner: Arc::new(RwLock::new(document)),
        })
    }

    pub async fn state(&self) -> AppDocument {
        self.inner.read().await.clone()
    }

    pub async fn rss_by_feed_slug(&self, feed_slug: &str) -> Result<String, StoreError> {
        let document = self.inner.read().await;
        let podcast = document
            .podcasts
            .iter()
            .find(|podcast| podcast.data.feed_slug == feed_slug)
            .ok_or(StoreError::NotFound("podcast feed"))?;

        Ok(build_rss_feed(
            &document.profile,
            podcast,
            &self.public_base_url,
        ))
    }

    pub async fn save_profile(&self, profile: Profile) -> Result<AppDocument, StoreError> {
        let (old_dir, new_dir, snapshot) = {
            let mut document = self.inner.write().await;
            let old_name = document.profile.name.clone();
            let new_name = profile.name.clone();
            let old_dir = self.resource_dir(&[&old_name]);
            let new_dir = self.resource_dir(&[&new_name]);
            document.profile = profile;
            rewrite_document_resource_urls(&mut document);
            document.revision += 1;
            (old_dir, new_dir, document.clone())
        };
        move_dir_if_needed(old_dir, new_dir).await?;
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
        let (old_dir, new_dir, podcast, snapshot) = {
            let mut document = self.inner.write().await;
            let profile_name = document.profile.name.clone();
            let podcast = find_podcast_mut(&mut document, podcast_id)?;
            let old_dir = self.resource_dir(&[&profile_name, &podcast.data.title]);
            let new_dir = self.resource_dir(&[&profile_name, &data.title]);
            podcast.data = data;
            rewrite_podcast_resource_urls(&profile_name, podcast);
            let podcast = podcast.clone();
            document.revision += 1;
            (old_dir, new_dir, podcast, document.clone())
        };
        move_dir_if_needed(old_dir, new_dir).await?;
        self.persist(&snapshot).await?;
        Ok(podcast)
    }

    pub async fn save_podcast_cover(
        &self,
        podcast_id: &str,
        file: UploadFile,
    ) -> Result<PodcastEntry, StoreError> {
        validate_extension(&file.file_name, IMAGE_EXTENSIONS)?;
        validate_size(file.size, MAX_IMAGE_BYTES)?;

        let (profile_name, podcast_title) = {
            let document = self.inner.read().await;
            let podcast = document
                .podcasts
                .iter()
                .find(|item| item.id == podcast_id)
                .ok_or(StoreError::NotFound("podcast"))?;
            (document.profile.name.clone(), podcast.data.title.clone())
        };

        let extension = file_extension(&file.file_name)?;
        let resource_path = self.resource_path(
            &[&profile_name, &podcast_title],
            &format!("cover.{extension}"),
        );
        let public_url = public_resource_url(&resource_path.relative_path);
        persist_upload(file, resource_path.full_path).await?;

        let (podcast, snapshot) = {
            let mut document = self.inner.write().await;
            let podcast = find_podcast_mut(&mut document, podcast_id)?;
            podcast.data.cover = Some(public_url);
            let podcast = podcast.clone();
            document.revision += 1;
            (podcast, document.clone())
        };
        self.persist(&snapshot).await?;
        Ok(podcast)
    }

    pub async fn delete_podcast(&self, podcast_id: &str) -> Result<AppDocument, StoreError> {
        let (removed_path, snapshot) = {
            let mut document = self.inner.write().await;
            let profile_name = document.profile.name.clone();
            let removed_path = document
                .podcasts
                .iter()
                .find(|podcast| podcast.id == podcast_id)
                .map(|podcast| self.resource_dir(&[&profile_name, &podcast.data.title]));
            let original_len = document.podcasts.len();
            document.podcasts.retain(|podcast| podcast.id != podcast_id);
            if document.podcasts.len() == original_len {
                return Err(StoreError::NotFound("podcast"));
            }
            document.revision += 1;
            (removed_path, document.clone())
        };
        if let Some(path) = removed_path {
            remove_dir_if_exists(path).await?;
        }
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
        let (old_dir, new_dir, episode, snapshot) = {
            let mut document = self.inner.write().await;
            let profile_name = document.profile.name.clone();
            let podcast = find_podcast_mut(&mut document, podcast_id)?;
            let podcast_title = podcast.data.title.clone();
            let current = podcast
                .episodes
                .iter_mut()
                .find(|item| item.id == episode_id)
                .ok_or(StoreError::NotFound("episode"))?;
            let old_dir = self.resource_dir(&[&profile_name, &podcast_title, &current.title]);
            let new_dir = self.resource_dir(&[&profile_name, &podcast_title, &episode.title]);
            *current = Episode {
                id: episode_id.to_string(),
                ..episode
            };
            rewrite_episode_resource_url(&profile_name, &podcast_title, current);
            renumber_episodes(&mut podcast.episodes);
            let episode = podcast
                .episodes
                .iter()
                .find(|item| item.id == episode_id)
                .cloned()
                .expect("updated episode should exist");
            document.revision += 1;
            (old_dir, new_dir, episode, document.clone())
        };
        move_dir_if_needed(old_dir, new_dir).await?;
        self.persist(&snapshot).await?;
        Ok(episode)
    }

    pub async fn save_episode_cover(
        &self,
        podcast_id: &str,
        episode_id: &str,
        file: UploadFile,
    ) -> Result<Episode, StoreError> {
        validate_extension(&file.file_name, IMAGE_EXTENSIONS)?;
        validate_size(file.size, MAX_IMAGE_BYTES)?;

        let (profile_name, podcast_title, episode_title) =
            self.resource_names(podcast_id, episode_id).await?;
        let extension = file_extension(&file.file_name)?;
        let resource_path = self.resource_path(
            &[&profile_name, &podcast_title, &episode_title],
            &format!("cover.{extension}"),
        );
        let public_url = public_resource_url(&resource_path.relative_path);
        persist_upload(file, resource_path.full_path).await?;

        let (episode, snapshot) = {
            let mut document = self.inner.write().await;
            let podcast = find_podcast_mut(&mut document, podcast_id)?;
            let episode = find_episode_mut(podcast, episode_id)?;
            episode.cover = Some(public_url);
            let episode = episode.clone();
            document.revision += 1;
            (episode, document.clone())
        };
        self.persist(&snapshot).await?;
        Ok(episode)
    }

    pub async fn save_episode_audio(
        &self,
        podcast_id: &str,
        episode_id: &str,
        file: UploadFile,
    ) -> Result<Episode, StoreError> {
        validate_extension(&file.file_name, AUDIO_EXTENSIONS)?;
        validate_size(file.size, MAX_AUDIO_BYTES)?;

        let (profile_name, podcast_title, episode_title) =
            self.resource_names(podcast_id, episode_id).await?;
        let file_name = sanitize_file_name(&file.file_name);
        let resource_path =
            self.resource_path(&[&profile_name, &podcast_title, &episode_title], &file_name);
        let file_size = file.size;
        persist_upload(file, resource_path.full_path).await?;

        let (episode, snapshot) = {
            let mut document = self.inner.write().await;
            let podcast = find_podcast_mut(&mut document, podcast_id)?;
            let episode = find_episode_mut(podcast, episode_id)?;
            episode.audio_file_name = Some(file_name);
            episode.audio_size = file_size;
            let episode = episode.clone();
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
        let (removed_path, podcast, snapshot) = {
            let mut document = self.inner.write().await;
            let profile_name = document.profile.name.clone();
            let podcast = find_podcast_mut(&mut document, podcast_id)?;
            let removed_path = podcast
                .episodes
                .iter()
                .find(|episode| episode.id == episode_id)
                .map(|episode| {
                    self.resource_dir(&[&profile_name, &podcast.data.title, &episode.title])
                });
            let original_len = podcast.episodes.len();
            podcast.episodes.retain(|episode| episode.id != episode_id);
            if podcast.episodes.len() == original_len {
                return Err(StoreError::NotFound("episode"));
            }
            renumber_episodes(&mut podcast.episodes);
            let podcast = podcast.clone();
            document.revision += 1;
            (removed_path, podcast, document.clone())
        };
        if let Some(path) = removed_path {
            remove_dir_if_exists(path).await?;
        }
        self.persist(&snapshot).await?;
        Ok(podcast)
    }

    async fn resource_names(
        &self,
        podcast_id: &str,
        episode_id: &str,
    ) -> Result<(String, String, String), StoreError> {
        let document = self.inner.read().await;
        let podcast = document
            .podcasts
            .iter()
            .find(|item| item.id == podcast_id)
            .ok_or(StoreError::NotFound("podcast"))?;
        let episode = podcast
            .episodes
            .iter()
            .find(|item| item.id == episode_id)
            .ok_or(StoreError::NotFound("episode"))?;
        Ok((
            document.profile.name.clone(),
            podcast.data.title.clone(),
            episode.title.clone(),
        ))
    }

    fn resource_dir(&self, segments: &[&str]) -> PathBuf {
        let mut path = self.resources_path.clone();
        for segment in segments {
            path.push(sanitize_path_segment(segment));
        }
        path
    }

    fn resource_path(&self, segments: &[&str], file_name: &str) -> ResourcePath {
        let mut full_path = self.resource_dir(segments);
        full_path.push(sanitize_file_name(file_name));

        let mut relative_path = PathBuf::new();
        for segment in segments {
            relative_path.push(sanitize_path_segment(segment));
        }
        relative_path.push(sanitize_file_name(file_name));

        ResourcePath {
            full_path,
            relative_path,
        }
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
    #[error("upload file is missing")]
    MissingUpload,
    #[error("unsupported file extension")]
    UnsupportedFile,
    #[error("file is too large")]
    FileTooLarge,
    #[error("invalid file name")]
    InvalidFileName,
    #[error(transparent)]
    Multipart(#[from] axum::extract::multipart::MultipartError),
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
            StoreError::MissingUpload
            | StoreError::UnsupportedFile
            | StoreError::FileTooLarge
            | StoreError::InvalidFileName
            | StoreError::Multipart(_) => StatusCode::BAD_REQUEST,
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
    let resources_path = store.resources_path.clone();
    Router::new()
        .route("/health", get(health))
        .route("/podcast/:feed_slug", get(get_podcast_rss))
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
        .route("/api/podcasts/:podcast_id/cover", put(put_podcast_cover))
        .route(
            "/api/podcasts/:podcast_id/episodes/:episode_id/cover",
            put(put_episode_cover),
        )
        .route(
            "/api/podcasts/:podcast_id/episodes/:episode_id/audio",
            put(put_episode_audio),
        )
        .nest_service("/resources", ServeDir::new(resources_path))
        .layer(DefaultBodyLimit::max(REQUEST_BODY_LIMIT))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(store)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true }))
}

async fn get_podcast_rss(
    State(store): State<Store>,
    Path(feed_slug): Path<String>,
) -> Result<Response, StoreError> {
    let rss = store.rss_by_feed_slug(&feed_slug).await?;
    Ok((
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/rss+xml; charset=utf-8"),
        )],
        rss,
    )
        .into_response())
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

async fn put_podcast_cover(
    State(store): State<Store>,
    Path(podcast_id): Path<String>,
    multipart: Multipart,
) -> Result<Json<PodcastEntry>, StoreError> {
    let file = collect_upload(multipart, MAX_IMAGE_BYTES).await?;
    store.save_podcast_cover(&podcast_id, file).await.map(Json)
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

async fn put_episode_cover(
    State(store): State<Store>,
    Path((podcast_id, episode_id)): Path<(String, String)>,
    multipart: Multipart,
) -> Result<Json<Episode>, StoreError> {
    let file = collect_upload(multipart, MAX_IMAGE_BYTES).await?;
    store
        .save_episode_cover(&podcast_id, &episode_id, file)
        .await
        .map(Json)
}

async fn put_episode_audio(
    State(store): State<Store>,
    Path((podcast_id, episode_id)): Path<(String, String)>,
    multipart: Multipart,
) -> Result<Json<Episode>, StoreError> {
    let file = collect_upload(multipart, MAX_AUDIO_BYTES).await?;
    store
        .save_episode_audio(&podcast_id, &episode_id, file)
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

fn find_episode_mut<'a>(
    podcast: &'a mut PodcastEntry,
    episode_id: &str,
) -> Result<&'a mut Episode, StoreError> {
    podcast
        .episodes
        .iter_mut()
        .find(|episode| episode.id == episode_id)
        .ok_or(StoreError::NotFound("episode"))
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

fn build_rss_feed(profile: &Profile, podcast: &PodcastEntry, public_base_url: &str) -> String {
    let title = fallback_title(&podcast.data.title, "Untitled podcast");
    let description = fallback_title(&podcast.data.description, "Podcast feed");
    let link = if podcast.data.website.trim().is_empty() {
        format!(
            "{public_base_url}/podcast/{}",
            url_path_escape(&podcast.data.feed_slug)
        )
    } else {
        podcast.data.website.clone()
    };

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(
        "<rss version=\"2.0\" xmlns:itunes=\"http://www.itunes.com/dtds/podcast-1.0.dtd\" xmlns:content=\"http://purl.org/rss/1.0/modules/content/\">\n",
    );
    xml.push_str("  <channel>\n");
    element(&mut xml, 4, "title", title);
    element(&mut xml, 4, "link", &link);
    element(&mut xml, 4, "description", description);
    element(
        &mut xml,
        4,
        "language",
        fallback_title(&podcast.data.language, "en"),
    );
    element(&mut xml, 4, "generator", "podcast_backend");
    element(&mut xml, 4, "itunes:explicit", "false");
    if !profile.name.trim().is_empty() {
        element(&mut xml, 4, "itunes:author", &profile.name);
    }
    if !profile.name.trim().is_empty() || !profile.email.trim().is_empty() {
        xml.push_str("    <itunes:owner>\n");
        if !profile.name.trim().is_empty() {
            element(&mut xml, 6, "itunes:name", &profile.name);
        }
        if !profile.email.trim().is_empty() {
            element(&mut xml, 6, "itunes:email", &profile.email);
        }
        xml.push_str("    </itunes:owner>\n");
    }
    if let Some(cover) = absolute_media_url(podcast.data.cover.as_deref(), public_base_url) {
        empty_element_attr(&mut xml, 4, "itunes:image", &[("href", cover.as_str())]);
    }
    for category in podcast_categories(podcast) {
        write_itunes_category(&mut xml, 4, &category);
    }

    for episode in podcast
        .episodes
        .iter()
        .filter(|episode| episode.audio_file_name.is_some())
    {
        write_episode_item(&mut xml, profile, podcast, episode, public_base_url);
    }

    xml.push_str("  </channel>\n");
    xml.push_str("</rss>\n");
    xml
}

fn write_episode_item(
    xml: &mut String,
    profile: &Profile,
    podcast: &PodcastEntry,
    episode: &Episode,
    public_base_url: &str,
) {
    let episode_title = fallback_title(&episode.title, "Untitled episode");
    let audio_file_name = episode
        .audio_file_name
        .as_deref()
        .expect("caller filters episodes without audio");
    let audio_url = absolute_url(
        &public_resource_url_from_segments(
            &[&profile.name, &podcast.data.title, &episode.title],
            audio_file_name,
        ),
        public_base_url,
    );
    let enclosure_type = mime_type_for(audio_file_name);
    let cover = episode
        .cover
        .as_deref()
        .or(podcast.data.cover.as_deref())
        .and_then(|value| absolute_media_url(Some(value), public_base_url));

    xml.push_str("    <item>\n");
    element(xml, 6, "title", episode_title);
    element(
        xml,
        6,
        "description",
        fallback_title(&episode.notes, episode_title),
    );
    element(
        xml,
        6,
        "content:encoded",
        fallback_title(&episode.notes, episode_title),
    );
    element(xml, 6, "guid", &episode.id);
    element(xml, 6, "itunes:episode", &episode.number.to_string());
    element(xml, 6, "itunes:episodeType", "full");
    if let Some(cover) = cover {
        empty_element_attr(xml, 6, "itunes:image", &[("href", cover.as_str())]);
    }
    empty_element_attr(
        xml,
        6,
        "enclosure",
        &[
            ("url", audio_url.as_str()),
            ("length", &episode.audio_size.to_string()),
            ("type", enclosure_type),
        ],
    );
    xml.push_str("    </item>\n");
}

fn podcast_categories(podcast: &PodcastEntry) -> Vec<String> {
    let mut categories = Vec::new();
    if !podcast.data.primary_category.trim().is_empty() {
        categories.push(podcast.data.primary_category.clone());
    }
    for category in &podcast.data.categories {
        if !category.trim().is_empty() && !categories.contains(category) {
            categories.push(category.clone());
        }
    }
    categories
}

fn write_itunes_category(xml: &mut String, indent: usize, category: &str) {
    let parts = category
        .split('>')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return;
    }
    if parts.len() == 1 {
        empty_element_attr(xml, indent, "itunes:category", &[("text", parts[0])]);
        return;
    }

    indent_spaces(xml, indent);
    xml.push_str("<itunes:category text=\"");
    xml.push_str(&xml_escape(parts[0]));
    xml.push_str("\">\n");
    empty_element_attr(xml, indent + 2, "itunes:category", &[("text", parts[1])]);
    indent_spaces(xml, indent);
    xml.push_str("</itunes:category>\n");
}

fn element(xml: &mut String, indent: usize, name: &str, value: &str) {
    indent_spaces(xml, indent);
    xml.push('<');
    xml.push_str(name);
    xml.push('>');
    xml.push_str(&xml_escape(value));
    xml.push_str("</");
    xml.push_str(name);
    xml.push_str(">\n");
}

fn empty_element_attr(xml: &mut String, indent: usize, name: &str, attrs: &[(&str, &str)]) {
    indent_spaces(xml, indent);
    xml.push('<');
    xml.push_str(name);
    for (key, value) in attrs {
        xml.push(' ');
        xml.push_str(key);
        xml.push_str("=\"");
        xml.push_str(&xml_escape(value));
        xml.push('"');
    }
    xml.push_str(" />\n");
}

fn indent_spaces(xml: &mut String, indent: usize) {
    for _ in 0..indent {
        xml.push(' ');
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn fallback_title<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn absolute_media_url(value: Option<&str>, public_base_url: &str) -> Option<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| absolute_url(value, public_base_url))
}

fn absolute_url(value: &str, public_base_url: &str) -> String {
    if value.starts_with("http://") || value.starts_with("https://") {
        return value.to_string();
    }
    if value.starts_with('/') {
        return format!("{public_base_url}{value}");
    }
    format!("{public_base_url}/{value}")
}

fn url_path_escape(value: &str) -> String {
    value.replace(' ', "%20")
}

fn mime_type_for(file_name: &str) -> &'static str {
    match file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("mp4") => "video/mp4",
        Some("aac") => "audio/aac",
        Some("wav") => "audio/wav",
        Some("aiff") => "audio/aiff",
        Some("flac") => "audio/flac",
        Some("ogg") => "audio/ogg",
        Some("mkv") => "video/x-matroska",
        _ => "application/octet-stream",
    }
}

#[derive(Debug)]
pub struct UploadFile {
    file_name: String,
    bytes: Vec<u8>,
    size: u64,
}

#[derive(Debug)]
struct ResourcePath {
    full_path: PathBuf,
    relative_path: PathBuf,
}

async fn collect_upload(mut multipart: Multipart, max_size: u64) -> Result<UploadFile, StoreError> {
    while let Some(mut field) = multipart.next_field().await? {
        if field.name() != Some("file") {
            continue;
        }

        let file_name = field
            .file_name()
            .map(ToOwned::to_owned)
            .ok_or(StoreError::InvalidFileName)?;
        let mut bytes = Vec::new();
        let mut size = 0_u64;

        while let Some(chunk) = field.chunk().await? {
            size += chunk.len() as u64;
            if size > max_size {
                return Err(StoreError::FileTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }

        return Ok(UploadFile {
            file_name,
            bytes,
            size,
        });
    }

    Err(StoreError::MissingUpload)
}

async fn persist_upload(file: UploadFile, path: PathBuf) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let temp_path = path.with_extension("upload.tmp");
    let mut output = tokio::fs::File::create(&temp_path).await?;
    output.write_all(&file.bytes).await?;
    output.flush().await?;
    drop(output);
    tokio::fs::rename(&temp_path, path).await?;
    Ok(())
}

async fn remove_dir_if_exists(path: PathBuf) -> Result<(), StoreError> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn move_dir_if_needed(from: PathBuf, to: PathBuf) -> Result<(), StoreError> {
    if from == to || !path_exists(&from).await? {
        return Ok(());
    }

    if let Some(parent) = to.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if path_exists(&to).await? {
        merge_dirs(from, to).await?;
    } else {
        tokio::fs::rename(from, to).await?;
    }
    Ok(())
}

async fn merge_dirs(from: PathBuf, to: PathBuf) -> Result<(), StoreError> {
    let mut entries = tokio::fs::read_dir(&from).await?;
    while let Some(entry) = entries.next_entry().await? {
        let target = to.join(entry.file_name());
        tokio::fs::rename(entry.path(), target).await?;
    }
    remove_dir_if_exists(from).await
}

async fn path_exists(path: &PathBuf) -> Result<bool, StoreError> {
    match tokio::fs::metadata(path).await {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn validate_size(size: u64, max_size: u64) -> Result<(), StoreError> {
    if size > max_size {
        return Err(StoreError::FileTooLarge);
    }
    Ok(())
}

fn validate_extension(file_name: &str, allowed: &[&str]) -> Result<(), StoreError> {
    let extension = file_extension(file_name)?;
    if allowed.contains(&extension.as_str()) {
        return Ok(());
    }
    Err(StoreError::UnsupportedFile)
}

fn file_extension(file_name: &str) -> Result<String, StoreError> {
    file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
        .ok_or(StoreError::InvalidFileName)
}

fn public_resource_url(relative_path: &std::path::Path) -> String {
    let path = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    format!("/resources/{path}")
}

fn rewrite_document_resource_urls(document: &mut AppDocument) {
    for podcast in &mut document.podcasts {
        rewrite_podcast_resource_urls(&document.profile.name, podcast);
    }
}

fn rewrite_podcast_resource_urls(profile_name: &str, podcast: &mut PodcastEntry) {
    if let Some(file_name) = podcast.data.cover.as_deref().and_then(resource_file_name) {
        podcast.data.cover = Some(public_resource_url_from_segments(
            &[profile_name, &podcast.data.title],
            &file_name,
        ));
    }

    for episode in &mut podcast.episodes {
        rewrite_episode_resource_url(profile_name, &podcast.data.title, episode);
    }
}

fn rewrite_episode_resource_url(profile_name: &str, podcast_title: &str, episode: &mut Episode) {
    if let Some(file_name) = episode.cover.as_deref().and_then(resource_file_name) {
        episode.cover = Some(public_resource_url_from_segments(
            &[profile_name, podcast_title, &episode.title],
            &file_name,
        ));
    }
}

fn public_resource_url_from_segments(segments: &[&str], file_name: &str) -> String {
    let mut path = PathBuf::new();
    for segment in segments {
        path.push(sanitize_path_segment(segment));
    }
    path.push(sanitize_file_name(file_name));
    public_resource_url(&path)
}

fn resource_file_name(value: &str) -> Option<String> {
    value
        .rsplit('/')
        .next()
        .map(sanitize_file_name)
        .filter(|file_name| !file_name.is_empty())
}

fn sanitize_path_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .filter_map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => Some(character),
            ' ' => Some('_'),
            _ => None,
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "Untitled".to_string()
    } else {
        sanitized
    }
}

fn sanitize_file_name(value: &str) -> String {
    let file_name = value
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("file")
        .chars()
        .filter_map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => Some(character),
            ' ' => Some('_'),
            _ => None,
        })
        .collect::<String>();

    if file_name.is_empty() {
        "file".to_string()
    } else {
        file_name
    }
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
    async fn upload_files_use_resource_tree_and_episode_delete_removes_files() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        let resources_path = dir.path().join("resurs");
        let store =
            Store::load_with_resources(&state_path, &resources_path, "https://example.test")
                .await
                .unwrap();

        store
            .save_profile(Profile {
                name: "Alice Smith".to_string(),
                email: "alice@example.com".to_string(),
            })
            .await
            .unwrap();
        let podcast = store
            .create_podcast(podcast_data("API Show"))
            .await
            .unwrap();
        let episode = store
            .save_episode(
                &podcast.id,
                &podcast.episodes[0].id,
                Episode {
                    title: "Launch Episode".to_string(),
                    ..podcast.episodes[0].clone()
                },
            )
            .await
            .unwrap();

        let episode = store
            .save_episode_audio(
                &podcast.id,
                &episode.id,
                UploadFile {
                    file_name: "hello world.mp3".to_string(),
                    bytes: b"audio".to_vec(),
                    size: 5,
                },
            )
            .await
            .unwrap();
        let episode = store
            .save_episode_cover(
                &podcast.id,
                &episode.id,
                UploadFile {
                    file_name: "cover.png".to_string(),
                    bytes: b"image".to_vec(),
                    size: 5,
                },
            )
            .await
            .unwrap();

        let episode_dir = resources_path
            .join("Alice_Smith")
            .join("API_Show")
            .join("Launch_Episode");
        assert!(episode_dir.join("hello_world.mp3").exists());
        assert!(episode_dir.join("cover.png").exists());
        assert_eq!(
            episode.cover,
            Some("/resources/Alice_Smith/API_Show/Launch_Episode/cover.png".to_string())
        );

        let rss = store.rss_by_feed_slug("good-show").await.unwrap();
        assert!(rss.contains("<rss version=\"2.0\""));
        assert!(rss.contains("xmlns:itunes=\"http://www.itunes.com/dtds/podcast-1.0.dtd\""));
        assert!(rss.contains("<title>API Show</title>"));
        assert!(rss.contains("<itunes:episode>1</itunes:episode>"));
        assert!(rss.contains("type=\"audio/mpeg\""));
        assert!(rss.contains(
            "url=\"https://example.test/resources/Alice_Smith/API_Show/Launch_Episode/hello_world.mp3\""
        ));

        store
            .delete_episode(&podcast.id, &episode.id)
            .await
            .unwrap();
        assert!(!episode_dir.exists());
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
