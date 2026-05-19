<script setup lang="ts">
import { computed, reactive, ref } from "vue";

type PodcastData = {
  title: string;
  description: string;
  website: string;
  feedSlug: string;
  cover: string | null;
  categories: string[];
  primaryCategory: string;
  language: string;
};

type Episode = {
  id: string;
  audioFileName: string | null;
  audioSize: number;
  title: string;
  notes: string;
  type: "Full";
  number: number;
  cover: string | null;
};

type PodcastEntry = {
  id: string;
  data: PodcastData;
  episodes: Episode[];
};

type Selection =
  | { kind: "podcast"; podcastId: string }
  | { kind: "episode"; podcastId: string; episodeId: string };

const categories = [
  "Arts",
  "Arts > Books",
  "Arts > Design",
  "Arts > Fashion & Beauty",
  "Arts > Food",
  "Arts > Performing Arts",
  "Arts > Visual Arts",
];

const languages = [
  { code: "en", label: "English" },
  { code: "ru", label: "Russian" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "it", label: "Italian" },
  { code: "pt", label: "Portuguese" },
  { code: "ja", label: "Japanese" },
  { code: "zh", label: "Chinese" },
];

const audioExt = ["wav", "mp3", "aac", "aiff", "mp4", "m4a", "flac", "ogg", "mkv"];
const maxAudio = 2 * 1024 * 1024 * 1024;

const profile = reactive({
  name: "",
  email: "",
});

function createId() {
  return crypto.randomUUID();
}

function emptyPodcast(): PodcastData {
  return {
    title: "",
    description: "",
    website: "",
    feedSlug: "",
    cover: null,
    categories: [],
    primaryCategory: "",
    language: "en",
  };
}

function emptyEpisode(number: number): Episode {
  return {
    id: createId(),
    audioFileName: null,
    audioSize: 0,
    title: "",
    notes: "",
    type: "Full",
    number,
    cover: null,
  };
}

function initialPodcast(): PodcastEntry {
  return {
    id: createId(),
    data: emptyPodcast(),
    episodes: [emptyEpisode(1)],
  };
}

const firstPodcast = initialPodcast();
const podcasts = ref<PodcastEntry[]>([firstPodcast]);
const selection = ref<Selection | null>({ kind: "podcast", podcastId: firstPodcast.id });
const expanded = ref<Record<string, boolean>>({ [firstPodcast.id]: true });
const youtubeFor = ref<string | null>(null);
const toast = ref("");
const coverError = ref("");
const audioError = ref("");
const savedProfileSnapshot = ref("");
const savedPodcastSnapshots = ref<Record<string, string>>({});
const savedEpisodeSnapshots = ref<Record<string, string>>({});

const selectedPodcast = computed(
  () =>
    selection.value
      ? podcasts.value.find((podcast) => podcast.id === selection.value?.podcastId) ?? null
      : null,
);

const selectedEpisode = computed(() => {
  const currentSelection = selection.value;
  if (!currentSelection || currentSelection.kind !== "episode" || !selectedPodcast.value) return null;
  return (
    selectedPodcast.value.episodes.find((episode) => episode.id === currentSelection.episodeId) ??
    null
  );
});

const youtubePodcast = computed(() =>
  youtubeFor.value
    ? podcasts.value.find((podcast) => podcast.id === youtubeFor.value) ?? null
    : null,
);

const rssUrl = computed(() =>
  selectedPodcast.value?.data.feedSlug
    ? `https://q1zin.ru/podcast/${selectedPodcast.value.data.feedSlug}`
    : "",
);

const profileDirty = computed(() => getProfileSnapshot() !== savedProfileSnapshot.value);
const selectedPodcastDirty = computed(() =>
  selectedPodcast.value ? isPodcastDirty(selectedPodcast.value) : false,
);
const selectedEpisodeDirty = computed(() =>
  selectedEpisode.value ? isEpisodeDirty(selectedEpisode.value) : false,
);

function showToast(message: string) {
  toast.value = message;
  window.setTimeout(() => {
    if (toast.value === message) toast.value = "";
  }, 2400);
}

function getProfileSnapshot() {
  return JSON.stringify({ name: profile.name, email: profile.email });
}

function getPodcastSnapshot(podcast: PodcastEntry) {
  return JSON.stringify(podcast.data);
}

function getEpisodeSnapshot(episode: Episode) {
  return JSON.stringify(episode);
}

function isPodcastDirty(podcast: PodcastEntry) {
  return savedPodcastSnapshots.value[podcast.id] !== getPodcastSnapshot(podcast);
}

function isEpisodeDirty(episode: Episode) {
  return savedEpisodeSnapshots.value[episode.id] !== getEpisodeSnapshot(episode);
}

function saveProfile() {
  savedProfileSnapshot.value = getProfileSnapshot();
  showToast("Profile saved");
  console.info("save:profile", { name: profile.name, email: profile.email });
}

function savePodcast(podcast: PodcastEntry) {
  savedPodcastSnapshots.value[podcast.id] = getPodcastSnapshot(podcast);
  showToast("Podcast saved");
  console.info("save:podcast", { id: podcast.id, data: podcast.data });
}

function saveEpisode(podcastId: string, episode: Episode) {
  savedEpisodeSnapshots.value[episode.id] = getEpisodeSnapshot(episode);
  showToast("Episode saved");
  console.info("save:episode", { podcastId, episode });
}

function addPodcast() {
  const podcast = initialPodcast();
  podcasts.value.push(podcast);
  expanded.value[podcast.id] = true;
  selection.value = { kind: "podcast", podcastId: podcast.id };
}

function addEpisode(podcastId: string) {
  const podcast = podcasts.value.find((item) => item.id === podcastId);
  if (!podcast) return;

  const episode = emptyEpisode(podcast.episodes.length + 1);
  podcast.episodes.push(episode);
  expanded.value[podcastId] = true;
  selection.value = { kind: "episode", podcastId, episodeId: episode.id };
}

function removePodcast(podcastId: string) {
  const podcastIndex = podcasts.value.findIndex((podcast) => podcast.id === podcastId);
  const podcast = podcasts.value[podcastIndex];
  if (!podcast) return;

  podcasts.value.splice(podcastIndex, 1);
  delete expanded.value[podcastId];
  delete savedPodcastSnapshots.value[podcastId];
  for (const episode of podcast.episodes) {
    delete savedEpisodeSnapshots.value[episode.id];
  }

  if (youtubeFor.value === podcastId) {
    youtubeFor.value = null;
  }

  const nextPodcast = podcasts.value[podcastIndex] ?? podcasts.value[podcastIndex - 1] ?? null;
  selection.value = nextPodcast ? { kind: "podcast", podcastId: nextPodcast.id } : null;
  showToast("Podcast deleted");
  console.info("delete:podcast", { id: podcastId });
}

function removeEpisode(podcastId: string, episodeId: string) {
  const podcast = podcasts.value.find((item) => item.id === podcastId);
  if (!podcast) return;

  podcast.episodes = podcast.episodes
    .filter((episode) => episode.id !== episodeId)
    .map((episode, index) => ({ ...episode, number: index + 1 }));
  delete savedEpisodeSnapshots.value[episodeId];

  if (selection.value?.kind === "episode" && selection.value.episodeId === episodeId) {
    selection.value = { kind: "podcast", podcastId };
  }

  showToast("Episode deleted");
  console.info("delete:episode", { podcastId, episodeId });
}

function toggleExpanded(podcastId: string) {
  expanded.value[podcastId] = !(expanded.value[podcastId] ?? true);
}

function isPodcastSelected(podcastId: string) {
  return selection.value?.kind === "podcast" && selection.value.podcastId === podcastId;
}

function isEpisodeSelected(episodeId: string) {
  return selection.value?.kind === "episode" && selection.value.episodeId === episodeId;
}

function setFeedSlug(value: string) {
  if (!selectedPodcast.value) return;
  selectedPodcast.value.data.feedSlug = value.replace(/[^a-z0-9-]/gi, "").toLowerCase();
}

function toggleCategory(category: string) {
  if (!selectedPodcast.value) return;

  const data = selectedPodcast.value.data;
  const hasCategory = data.categories.includes(category);
  data.categories = hasCategory
    ? data.categories.filter((item) => item !== category)
    : [...data.categories, category];
  if (!data.categories.includes(data.primaryCategory)) {
    data.primaryCategory = data.categories[0] ?? "";
  }
}

async function copyRss() {
  if (!rssUrl.value) {
    showToast("Set the podcast feed slug first");
    return;
  }

  try {
    await navigator.clipboard.writeText(rssUrl.value);
    showToast("RSS URL copied");
  } catch {
    showToast("Failed to copy RSS URL");
  }
}

function getInputFile(event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0] ?? null;
  input.value = "";
  return file;
}

function handleCoverUpload(event: Event, target: "podcast" | "episode") {
  const file = getInputFile(event);
  coverError.value = "";
  if (!file) return;

  if (!["image/jpeg", "image/png"].includes(file.type)) {
    coverError.value = "File must be JPG or PNG";
    return;
  }

  if (file.size > 5 * 1024 * 1024) {
    coverError.value = "File must be under 5MB";
    return;
  }

  const reader = new FileReader();
  reader.onload = () => {
    if (target === "podcast" && selectedPodcast.value) {
      selectedPodcast.value.data.cover = String(reader.result);
    }
    if (target === "episode" && selectedEpisode.value) {
      selectedEpisode.value.cover = String(reader.result);
    }
  };
  reader.readAsDataURL(file);
}

function clearCover(target: "podcast" | "episode") {
  if (target === "podcast" && selectedPodcast.value) {
    selectedPodcast.value.data.cover = null;
  }
  if (target === "episode" && selectedEpisode.value) {
    selectedEpisode.value.cover = null;
  }
}

function handleAudioUpload(event: Event) {
  const file = getInputFile(event);
  audioError.value = "";
  if (!file || !selectedEpisode.value) return;

  const extension = file.name.split(".").pop()?.toLowerCase() ?? "";
  if (!audioExt.includes(extension)) {
    audioError.value = `Unsupported format .${extension}`;
    return;
  }

  if (file.size > maxAudio) {
    audioError.value = "File exceeds 2GB limit";
    return;
  }

  selectedEpisode.value.audioFileName = file.name;
  selectedEpisode.value.audioSize = file.size;
}

function formatSize(bytes: number) {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}
</script>

<template>
  <div class="app-shell">
    <header class="profile-bar">
      <section class="profile-card" aria-label="Profile">
        <div class="profile-title">
          <span class="icon-badge" aria-hidden="true">@</span>
          <div>
            <h1>Profile</h1>
            <p>Podcast workspace</p>
            <span class="status-pill" :class="{ dirty: profileDirty }">
              {{ profileDirty ? "Unsaved changes" : "Saved" }}
            </span>
          </div>
        </div>
        <label class="field compact">
          <span>Name</span>
          <input v-model="profile.name" placeholder="Your name" />
        </label>
        <label class="field compact">
          <span>Email</span>
          <input v-model="profile.email" type="email" placeholder="you@example.com" />
        </label>
        <button
          class="button save-button"
          type="button"
          :disabled="!profileDirty"
          @click="saveProfile"
        >
          Save profile
        </button>
      </section>
    </header>

    <div class="workspace">
      <aside class="sidebar" aria-label="Podcasts">
        <div class="sidebar-head">
          <strong>Podcasts</strong>
          <button class="button ghost small" type="button" @click="addPodcast">
            <span aria-hidden="true">+</span>
            New
          </button>
        </div>

        <nav class="podcast-tree">
          <article v-for="podcast in podcasts" :key="podcast.id" class="tree-group">
            <div class="tree-row" :class="{ active: isPodcastSelected(podcast.id) }">
              <button class="icon-button" type="button" @click="toggleExpanded(podcast.id)">
                {{ expanded[podcast.id] ?? true ? "v" : ">" }}
              </button>
              <button
                class="tree-main"
                type="button"
                @click="selection = { kind: 'podcast', podcastId: podcast.id }"
              >
                <span class="tree-icon" aria-hidden="true">o</span>
                <span>{{ podcast.data.title || "Untitled podcast" }}</span>
              </button>
              <button
                class="icon-button add-inline"
                type="button"
                title="Add episode"
                @click="addEpisode(podcast.id)"
              >
                +
              </button>
              <button
                class="icon-button danger add-inline"
                type="button"
                title="Delete podcast"
                @click="removePodcast(podcast.id)"
              >
                x
              </button>
            </div>

            <div v-if="expanded[podcast.id] ?? true" class="episode-list">
              <div v-for="episode in podcast.episodes" :key="episode.id" class="episode-row">
                <button
                  class="episode-link"
                  :class="{ active: isEpisodeSelected(episode.id) }"
                  type="button"
                  @click="
                    selection = {
                      kind: 'episode',
                      podcastId: podcast.id,
                      episodeId: episode.id,
                    }
                  "
                >
                  <span aria-hidden="true">*</span>
                  <span>{{ episode.number }}. {{ episode.title || "Untitled episode" }}</span>
                </button>
                <button
                  class="icon-button danger add-inline"
                  type="button"
                  title="Delete episode"
                  @click="removeEpisode(podcast.id, episode.id)"
                >
                  x
                </button>
              </div>
              <button class="episode-link muted" type="button" @click="addEpisode(podcast.id)">
                <span aria-hidden="true">+</span>
                Add episode
              </button>
            </div>
          </article>
        </nav>
      </aside>

      <main class="content">
        <section v-if="!selectedPodcast" class="panel empty-panel">
          <h2>No podcast selected</h2>
          <p>Create a new podcast from the sidebar to start editing.</p>
          <button class="button" type="button" @click="addPodcast">New podcast</button>
        </section>

        <section v-if="selectedPodcast && !selectedEpisode" class="panel">
          <div class="panel-head">
            <div>
              <h2>Podcast details</h2>
              <span class="status-pill" :class="{ dirty: selectedPodcastDirty }">
                {{ selectedPodcastDirty ? "Unsaved changes" : "Saved" }}
              </span>
            </div>
            <div class="actions">
              <button
                class="button save-button small"
                type="button"
                :disabled="!selectedPodcastDirty"
                @click="savePodcast(selectedPodcast)"
              >
                Save podcast
              </button>
              <button class="button outline small" type="button" @click="copyRss">
                <span aria-hidden="true">))</span>
                RSS
              </button>
              <button class="button outline small" type="button" @click="youtubeFor = selectedPodcast.id">
                <span class="youtube-mark" aria-hidden="true">></span>
                Pub to YouTube
              </button>
              <button
                class="button danger-outline small"
                type="button"
                @click="removePodcast(selectedPodcast.id)"
              >
                Delete podcast
              </button>
            </div>
          </div>

          <div class="form-grid">
            <label class="field span-2">
              <span>Title</span>
              <input v-model="selectedPodcast.data.title" placeholder="My awesome podcast" />
            </label>

            <label class="field span-2">
              <span>Description</span>
              <textarea
                v-model="selectedPodcast.data.description"
                rows="4"
                placeholder="What is your podcast about?"
              />
            </label>

            <label class="field">
              <span>Public Website</span>
              <input v-model="selectedPodcast.data.website" placeholder="https://example.com" />
            </label>

            <label class="field">
              <span>Podcast Feed</span>
              <div class="feed-input">
                <span>https://q1zin.ru/podcast/</span>
                <input
                  :value="selectedPodcast.data.feedSlug"
                  placeholder="name"
                  @input="setFeedSlug(($event.target as HTMLInputElement).value)"
                />
              </div>
            </label>
          </div>

          <section class="upload-row">
            <div class="cover-preview">
              <img v-if="selectedPodcast.data.cover" :src="selectedPodcast.data.cover" alt="Podcast cover" />
              <span v-else aria-hidden="true">[]</span>
              <button
                v-if="selectedPodcast.data.cover"
                class="remove-cover"
                type="button"
                title="Remove cover"
                @click="clearCover('podcast')"
              >
                x
              </button>
            </div>
            <div class="upload-copy">
              <h3>Cover Art</h3>
              <label class="button outline">
                {{ selectedPodcast.data.cover ? "Replace image" : "Upload image" }}
                <input type="file" accept="image/jpeg,image/png" @change="handleCoverUpload($event, 'podcast')" />
              </label>
              <p>Square images work best. JPG or PNG, under 5MB. Recommended: 3000 x 3000 px.</p>
              <p v-if="coverError" class="error">{{ coverError }}</p>
            </div>
          </section>

          <section class="field span-2">
            <span>Categories</span>
            <div class="category-box">
              <label v-for="category in categories" :key="category" class="check-row">
                <input
                  type="checkbox"
                  :checked="selectedPodcast.data.categories.includes(category)"
                  @change="toggleCategory(category)"
                />
                <span>{{ category }}</span>
              </label>
            </div>
            <div v-if="selectedPodcast.data.categories.length" class="chips">
              <button
                v-for="category in selectedPodcast.data.categories"
                :key="category"
                class="chip"
                type="button"
                @click="toggleCategory(category)"
              >
                {{ category }} x
              </button>
            </div>
          </section>

          <div class="form-grid">
            <label class="field">
              <span>Primary Category</span>
              <select v-model="selectedPodcast.data.primaryCategory" :disabled="!selectedPodcast.data.categories.length">
                <option value="">Pick a primary category</option>
                <option v-for="category in selectedPodcast.data.categories" :key="category" :value="category">
                  {{ category }}
                </option>
              </select>
            </label>

            <label class="field">
              <span>Language</span>
              <select v-model="selectedPodcast.data.language">
                <option v-for="language in languages" :key="language.code" :value="language.code">
                  {{ language.label }}
                </option>
              </select>
            </label>
          </div>
        </section>

        <section v-if="selectedPodcast && selectedEpisode" class="panel">
          <div class="panel-head">
            <div>
              <h2>Episode {{ selectedEpisode.number }}</h2>
              <span class="status-pill" :class="{ dirty: selectedEpisodeDirty }">
                {{ selectedEpisodeDirty ? "Unsaved changes" : "Saved" }}
              </span>
            </div>
            <div class="actions">
              <button
                class="button save-button small"
                type="button"
                :disabled="!selectedEpisodeDirty"
                @click="saveEpisode(selectedPodcast.id, selectedEpisode)"
              >
                Save episode
              </button>
              <button
                class="button danger-outline small"
                type="button"
                @click="removeEpisode(selectedPodcast.id, selectedEpisode.id)"
              >
                Delete episode
              </button>
            </div>
          </div>

          <section class="audio-drop">
            <label class="button outline">
              {{ selectedEpisode.audioFileName ? "Replace audio" : "Upload audio" }}
              <input
                type="file"
                accept=".wav,.mp3,.aac,.aiff,.mp4,.m4a,.flac,.ogg,.mkv,audio/*,video/mp4,video/x-matroska"
                @change="handleAudioUpload"
              />
            </label>
            <span v-if="selectedEpisode.audioFileName">
              {{ selectedEpisode.audioFileName }} - {{ formatSize(selectedEpisode.audioSize) }}
            </span>
            <span v-else>Up to 2GB - wav, mp3, aac, aiff, mp4, m4a, flac, ogg, mkv</span>
          </section>
          <p v-if="audioError" class="error">{{ audioError }}</p>

          <div class="form-grid">
            <label class="field">
              <span>Episode Title</span>
              <input v-model="selectedEpisode.title" placeholder="My amazing episode" />
            </label>
            <label class="field">
              <span>Episode Number</span>
              <input v-model.number="selectedEpisode.number" min="1" type="number" />
            </label>
            <label class="field">
              <span>Type of episode</span>
              <select v-model="selectedEpisode.type">
                <option value="Full">Full</option>
              </select>
            </label>
          </div>

          <label class="field">
            <span>Episode Notes</span>
            <textarea v-model="selectedEpisode.notes" rows="4" placeholder="Show notes, links, timestamps..." />
          </label>

          <section class="upload-row">
            <div class="cover-preview">
              <img v-if="selectedEpisode.cover" :src="selectedEpisode.cover" alt="Episode cover" />
              <span v-else aria-hidden="true">[]</span>
              <button
                v-if="selectedEpisode.cover"
                class="remove-cover"
                type="button"
                title="Remove cover"
                @click="clearCover('episode')"
              >
                x
              </button>
            </div>
            <div class="upload-copy">
              <h3>Episode Cover Art</h3>
              <label class="button outline">
                {{ selectedEpisode.cover ? "Replace image" : "Upload image" }}
                <input type="file" accept="image/jpeg,image/png" @change="handleCoverUpload($event, 'episode')" />
              </label>
              <p>Square images work best. JPG or PNG, under 5MB. Recommended: 3000 x 3000 px.</p>
              <p v-if="coverError" class="error">{{ coverError }}</p>
            </div>
          </section>
        </section>
      </main>

      <aside v-if="youtubePodcast" class="youtube-panel">
        <div class="sidebar-head">
          <strong><span class="youtube-mark" aria-hidden="true">></span> Publish to YouTube</strong>
          <button class="icon-button" type="button" @click="youtubeFor = null">x</button>
        </div>
        <div class="youtube-empty">
          <span class="youtube-big" aria-hidden="true">></span>
          <p>
            YouTube publish window for
            <strong>{{ youtubePodcast.data.title || "this podcast" }}</strong>
            will open here.
          </p>
          <p>Integration coming soon.</p>
        </div>
      </aside>
    </div>

    <div v-if="toast" class="toast">{{ toast }}</div>
  </div>
</template>

<style>
:root {
  font-family:
    Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  color: #17171c;
  background: #f4f4f6;
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  min-width: 320px;
  min-height: 100vh;
}

button,
input,
select,
textarea {
  font: inherit;
}

button {
  cursor: pointer;
}

.app-shell {
  display: flex;
  min-height: 100vh;
  flex-direction: column;
  background: #f3f3f5;
}

.profile-bar {
  border-bottom: 1px solid rgba(15, 15, 20, 0.1);
  background: #ffffff;
}

.profile-card {
  display: grid;
  max-width: 1120px;
  grid-template-columns: minmax(190px, 1fr) minmax(180px, 240px) minmax(210px, 280px) auto;
  gap: 16px;
  align-items: end;
  margin: 0 auto;
  padding: 16px;
}

.profile-title {
  display: flex;
  align-items: center;
  gap: 12px;
}

.profile-title h1,
.profile-title p,
.panel h2,
.upload-copy h3,
.youtube-empty p {
  margin: 0;
}

.profile-title h1,
.panel h2 {
  font-size: 20px;
  font-weight: 650;
  line-height: 1.35;
}

.profile-title p,
.upload-copy p,
.audio-drop span,
.episode-link.muted,
.youtube-empty {
  color: #717182;
}

.icon-badge {
  display: grid;
  width: 40px;
  height: 40px;
  place-items: center;
  border-radius: 8px;
  background: #ececf0;
  color: #030213;
}

.workspace {
  display: flex;
  min-height: 0;
  flex: 1;
  overflow: hidden;
}

.sidebar,
.youtube-panel {
  display: flex;
  width: 288px;
  min-width: 248px;
  flex-direction: column;
  border-right: 1px solid rgba(15, 15, 20, 0.1);
  background: #ffffff;
}

.youtube-panel {
  width: 384px;
  border-right: 0;
  border-left: 1px solid rgba(15, 15, 20, 0.1);
}

.sidebar-head {
  display: flex;
  min-height: 57px;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border-bottom: 1px solid rgba(15, 15, 20, 0.1);
  padding: 12px 16px;
}

.podcast-tree {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.tree-group {
  margin-bottom: 4px;
}

.tree-row,
.episode-row,
.episode-link {
  display: flex;
  min-height: 34px;
  align-items: center;
  gap: 6px;
  border-radius: 8px;
  color: #1f2028;
}

.tree-row {
  padding: 4px 6px;
}

.episode-row {
  border-radius: 8px;
}

.tree-row:hover,
.tree-row.active,
.episode-link:hover,
.episode-link.active {
  background: #e9ebef;
}

.tree-main,
.episode-link {
  min-width: 0;
  flex: 1;
  border: 0;
  background: transparent;
  text-align: left;
}

.tree-main {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0;
}

.tree-main span:last-child,
.episode-link span:last-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tree-icon {
  color: #030213;
}

.add-inline {
  opacity: 0;
  transition: opacity 160ms ease;
}

.tree-row:hover .add-inline {
  opacity: 1;
}

.episode-row:hover .add-inline {
  opacity: 1;
}

.episode-list {
  display: grid;
  gap: 2px;
  margin: 2px 0 4px 28px;
  border-left: 1px solid rgba(15, 15, 20, 0.1);
  padding-left: 8px;
}

.episode-link {
  padding: 6px 8px;
}

.content {
  flex: 1;
  overflow-y: auto;
  padding: 24px;
}

.panel {
  display: grid;
  max-width: 768px;
  gap: 24px;
  margin: 0 auto;
  border: 1px solid rgba(15, 15, 20, 0.1);
  border-radius: 8px;
  background: #ffffff;
  padding: 24px;
  box-shadow: 0 12px 34px rgba(16, 17, 25, 0.05);
}

.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

.empty-panel {
  align-content: start;
  max-width: 560px;
}

.empty-panel p {
  margin: 0;
  color: #717182;
}

.status-pill {
  display: inline-flex;
  width: fit-content;
  align-items: center;
  min-height: 24px;
  margin-top: 6px;
  border: 1px solid rgba(15, 15, 20, 0.1);
  border-radius: 999px;
  background: #edf7ee;
  color: #287a34;
  padding: 2px 9px;
  font-size: 12px;
  font-weight: 650;
  line-height: 1.2;
}

.status-pill.dirty {
  background: #fff7e8;
  color: #8a5a00;
}

.actions,
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}

.span-2 {
  grid-column: 1 / -1;
}

.field {
  display: grid;
  gap: 8px;
  min-width: 0;
  color: #17171c;
  font-weight: 550;
}

.field.compact {
  gap: 6px;
}

.field span,
.upload-copy h3 {
  font-size: 14px;
  line-height: 1.35;
}

.field input,
.field select,
.field textarea,
.feed-input {
  width: 100%;
  min-height: 40px;
  border: 1px solid rgba(15, 15, 20, 0.12);
  border-radius: 8px;
  background: #f7f7f8;
  color: #17171c;
  outline: none;
  transition:
    border-color 160ms ease,
    box-shadow 160ms ease,
    background 160ms ease;
}

.field input,
.field select,
.field textarea {
  padding: 9px 12px;
}

.field textarea {
  min-height: 112px;
  resize: vertical;
}

.field input:focus,
.field select:focus,
.field textarea:focus,
.feed-input:focus-within {
  border-color: #8a8d98;
  background: #ffffff;
  box-shadow: 0 0 0 3px rgba(3, 2, 19, 0.08);
}

.feed-input {
  display: flex;
  align-items: stretch;
  overflow: hidden;
}

.feed-input > span {
  display: flex;
  align-items: center;
  flex: 0 0 auto;
  background: #ececf0;
  padding: 0 12px;
  color: #717182;
  font-weight: 400;
}

.feed-input input {
  min-width: 80px;
  border: 0;
  background: transparent;
  box-shadow: none;
}

.button,
.icon-button,
.chip {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 38px;
  border-radius: 8px;
  border: 1px solid transparent;
  background: #030213;
  color: #ffffff;
  padding: 8px 12px;
  font-weight: 600;
  text-decoration: none;
  transition:
    background 160ms ease,
    border-color 160ms ease,
    color 160ms ease;
}

.button.small {
  min-height: 32px;
  padding: 5px 10px;
}

.button.ghost,
.icon-button {
  background: transparent;
  color: #17171c;
}

.button.outline {
  border-color: rgba(15, 15, 20, 0.14);
  background: #ffffff;
  color: #17171c;
}

.button.save-button {
  background: #1f6f43;
  color: #ffffff;
}

.button.danger-outline {
  border-color: rgba(212, 24, 61, 0.32);
  background: #ffffff;
  color: #b51032;
}

.button:hover,
.chip:hover {
  background: #1f1d31;
}

.button.ghost:hover,
.button.outline:hover,
.button.danger-outline:hover,
.icon-button:hover {
  border-color: rgba(15, 15, 20, 0.14);
  background: #e9ebef;
  color: #17171c;
}

.button.save-button:hover {
  background: #185936;
}

.button.danger-outline:hover {
  border-color: rgba(212, 24, 61, 0.48);
  background: #fff0f2;
  color: #9b0f2b;
}

.button:disabled {
  cursor: not-allowed;
  border-color: rgba(15, 15, 20, 0.08);
  background: #d9dbe0;
  color: #777b86;
}

.button input {
  display: none;
}

.icon-button {
  width: 28px;
  height: 28px;
  min-height: 28px;
  padding: 0;
}

.icon-button.danger:hover {
  background: #fff0f2;
  color: #d4183d;
}

.youtube-mark {
  color: #d4183d;
}

.upload-row {
  display: flex;
  align-items: flex-start;
  gap: 16px;
}

.cover-preview {
  position: relative;
  display: grid;
  width: 160px;
  min-width: 160px;
  aspect-ratio: 1;
  place-items: center;
  overflow: hidden;
  border: 1px solid rgba(15, 15, 20, 0.1);
  border-radius: 8px;
  background:
    linear-gradient(135deg, rgba(3, 2, 19, 0.08), transparent),
    #ececf0;
  color: #717182;
  font-size: 42px;
}

.cover-preview img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.remove-cover {
  position: absolute;
  top: 6px;
  right: 6px;
  display: grid;
  width: 28px;
  height: 28px;
  place-items: center;
  border: 1px solid rgba(15, 15, 20, 0.14);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.92);
  color: #17171c;
}

.upload-copy {
  display: grid;
  gap: 8px;
  min-width: 0;
}

.upload-copy p {
  max-width: 430px;
  line-height: 1.45;
}

.category-box {
  display: grid;
  max-height: 220px;
  overflow-y: auto;
  border: 1px solid rgba(15, 15, 20, 0.1);
  border-radius: 8px;
  padding: 8px;
}

.check-row {
  display: flex;
  align-items: center;
  gap: 9px;
  border-radius: 6px;
  padding: 8px;
  font-weight: 400;
}

.check-row:hover {
  background: #e9ebef;
}

.check-row input {
  width: 16px;
  height: 16px;
  accent-color: #030213;
}

.chip {
  min-height: 28px;
  border-color: transparent;
  background: #ececf0;
  color: #17171c;
  padding: 4px 9px;
  font-size: 13px;
}

.audio-drop {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  border: 1px dashed rgba(15, 15, 20, 0.22);
  border-radius: 8px;
  padding: 16px;
}

.error {
  margin: 0;
  color: #d4183d;
}

.youtube-empty {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 28px;
  text-align: center;
  line-height: 1.5;
}

.youtube-empty strong {
  color: #17171c;
}

.youtube-big {
  display: grid;
  width: 64px;
  height: 64px;
  place-items: center;
  border-radius: 8px;
  background: #fff0f2;
  color: #d4183d;
  font-size: 28px;
}

.toast {
  position: fixed;
  top: 18px;
  right: 18px;
  z-index: 10;
  max-width: min(320px, calc(100vw - 36px));
  border: 1px solid rgba(15, 15, 20, 0.1);
  border-radius: 8px;
  background: #17171c;
  color: #ffffff;
  padding: 10px 14px;
  box-shadow: 0 14px 34px rgba(16, 17, 25, 0.18);
}

@media (max-width: 980px) {
  .profile-card {
    grid-template-columns: 1fr 1fr;
  }

  .profile-title {
    grid-column: 1 / -1;
  }

  .workspace {
    flex-direction: column;
    overflow: visible;
  }

  .sidebar,
  .youtube-panel {
    width: 100%;
    min-width: 0;
    max-height: 320px;
    border-right: 0;
    border-bottom: 1px solid rgba(15, 15, 20, 0.1);
  }

  .youtube-panel {
    max-height: none;
    border-left: 0;
  }

  .content {
    overflow: visible;
  }
}

@media (max-width: 680px) {
  .profile-card,
  .form-grid {
    grid-template-columns: 1fr;
  }

  .span-2 {
    grid-column: auto;
  }

  .profile-card,
  .content,
  .panel {
    padding: 16px;
  }

  .panel-head,
  .upload-row {
    flex-direction: column;
  }

  .actions {
    width: 100%;
  }

  .actions .button {
    flex: 1;
  }

  .cover-preview {
    width: 140px;
    min-width: 140px;
  }

  .feed-input {
    display: grid;
  }

  .feed-input > span {
    min-height: 38px;
  }
}
</style>
