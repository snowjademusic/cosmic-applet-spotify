use cosmic::app::{Core, Task};
use cosmic::iced::{
    self,
    platform_specific::shell::commands::popup,
    widget::{container, image as iced_image, row, text},
    window, Alignment, Length,
};
use cosmic::widget::{list_column, settings, toggler};
use cosmic::{applet, executor, Element};
use mpris::{PlaybackStatus, Player, PlayerFinder};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

const APP_ID: &str = "io.github.snowjademusic.CosmicAppletSpotify";
const POLL_INTERVAL_SECONDS: u64 = 3;
const PREFS_FILE_NAME: &str = "panel-visibility.conf";

#[derive(Clone, Copy, Debug)]
struct PanelVisibility {
    show_title: bool,
    show_artists: bool,
    show_artwork: bool,
}

// comment to trigger release again (atp idek)

impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            show_title: true,
            show_artists: true,
            show_artwork: true,
        }
    }
}

#[derive(Clone, Debug)]
struct TrackInfo {
    title: String,
    artists: String,
    art_url: Option<String>,
    media_url: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    TogglePopup,
    PopupClosed(window::Id),
    RefreshNowPlaying,
    NowPlayingLoaded(Option<TrackInfo>),
    AlbumArtFetched(Option<Arc<Vec<u8>>>, String),
    ToggleShowTitle(bool),
    ToggleShowArtists(bool),
    ToggleShowArtwork(bool),
}

struct SpotifyApplet {
    core: Core,
    popup: Option<window::Id>,
    track: Option<TrackInfo>,
    album_art: Option<Arc<Vec<u8>>>,
    art_url_loaded: Option<String>,
    show_title: bool,
    show_artists: bool,
    show_artwork: bool,
}

impl Default for SpotifyApplet {
    fn default() -> Self {
        let visibility = load_panel_visibility().unwrap_or_default();
        Self {
            core: Core::default(),
            popup: None,
            track: None,
            album_art: None,
            art_url_loaded: None,
            show_title: visibility.show_title,
            show_artists: visibility.show_artists,
            show_artwork: visibility.show_artwork,
        }
    }
}

impl SpotifyApplet {
    fn panel_visibility(&self) -> PanelVisibility {
        PanelVisibility {
            show_title: self.show_title,
            show_artists: self.show_artists,
            show_artwork: self.show_artwork,
        }
    }

    fn persist_panel_visibility(&self) {
        let _ = save_panel_visibility(self.panel_visibility());
    }
}

impl cosmic::Application for SpotifyApplet {
    type Executor = executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        (
            Self {
                core,
                ..Default::default()
            },
            Task::perform(fetch_now_playing(), |track| {
                cosmic::Action::App(Message::NowPlayingLoaded(track))
            }),
        )
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::time::every(std::time::Duration::from_secs(POLL_INTERVAL_SECONDS))
            .map(|_| Message::RefreshNowPlaying)
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::TogglePopup => {
                if let Some(id) = self.popup.take() {
                    return popup::destroy_popup(id);
                }
                let new_id = window::Id::unique();
                self.popup = Some(new_id);
                let popup_settings = self.core.applet.get_popup_settings(
                    self.core.main_window_id().unwrap(),
                    new_id,
                    Some((360, 160)),
                    None,
                    None,
                );
                popup::get_popup(popup_settings)
            }

            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
                Task::none()
            }

            Message::RefreshNowPlaying => Task::perform(fetch_now_playing(), |track| {
                cosmic::Action::App(Message::NowPlayingLoaded(track))
            }),

            Message::NowPlayingLoaded(track) => {
                let art_key = track.as_ref().and_then(art_cache_key);
                let needs_art = art_key.is_some()
                    && (art_key.as_deref() != self.art_url_loaded.as_deref()
                        || self.album_art.is_none());
                self.track = track;

                if art_key.is_none() {
                    self.album_art = None;
                    self.art_url_loaded = None;
                    return Task::none();
                }

                if needs_art {
                    let key = art_key.unwrap();
                    let track = self.track.clone().unwrap();
                    Task::perform(fetch_album_art_for_track(track), move |bytes| {
                        cosmic::Action::App(Message::AlbumArtFetched(
                            bytes.map(Arc::new),
                            key.clone(),
                        ))
                    })
                } else {
                    Task::none()
                }
            }

            Message::AlbumArtFetched(bytes, url) => {
                self.album_art = bytes;
                self.art_url_loaded = Some(url);
                Task::none()
            }

            Message::ToggleShowTitle(value) => {
                self.show_title = value;
                self.persist_panel_visibility();
                Task::none()
            }

            Message::ToggleShowArtists(value) => {
                self.show_artists = value;
                self.persist_panel_visibility();
                Task::none()
            }

            Message::ToggleShowArtwork(value) => {
                self.show_artwork = value;
                self.persist_panel_visibility();
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let icon_size = self.core.applet.suggested_size(false).0 as f32;

        let raw_title = self
            .track
            .as_ref()
            .map(|t| t.title.trim())
            .filter(|s| !s.is_empty())
            .map(|s| shorten(s, 24));
        let raw_artist = self
            .track
            .as_ref()
            .map(|t| t.artists.trim())
            .filter(|s| !s.is_empty())
            .map(|s| shorten(s, 24));

        let title_part = if self.show_title { raw_title } else { None };
        let artist_part = if self.show_artists { raw_artist } else { None };

        let label = match (title_part, artist_part) {
            (Some(title), Some(artist)) => format!("{} • {}", title, artist),
            (Some(title), None) => title,
            (None, Some(artist)) => artist,
            (None, None) => String::new(),
        };

        let has_known_art = self.show_artwork && self.album_art.is_some();
        let show_music_note_fallback = !has_known_art && label.is_empty();

        let art: Option<Element<'_, Message>> = if has_known_art {
            self.album_art.as_ref().map(|bytes| {
                let handle = iced_image::Handle::from_bytes(bytes.as_ref().clone());
                iced_image::Image::new(handle)
                    .width(Length::Fixed(icon_size))
                    .height(Length::Fixed(icon_size))
                    .into()
            })
        } else if show_music_note_fallback {
            Some(
                container(text("♫"))
                    .width(Length::Fixed(icon_size))
                    .height(Length::Fixed(icon_size))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .into(),
            )
        } else {
            None
        };

        let mut panel_row = row![].spacing(8).align_y(Alignment::Center);
        if let Some(art) = art {
            panel_row = panel_row.push(art);
        }
        panel_row = panel_row.push(text(label));

        self.core
            .applet
            .autosize_window(
                container(
                    cosmic::widget::button::custom(panel_row)
                        .on_press_down(Message::TogglePopup)
                        .class(cosmic::theme::Button::AppletIcon),
                )
                .padding([0, 10]),
            )
            .into()
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Self::Message> {
        let content = list_column()
            .padding(8)
            .spacing(0)
            .add(settings::item(
                "Show song title",
                toggler(self.show_title).on_toggle(Message::ToggleShowTitle),
            ))
            .add(settings::item(
                "Show artists",
                toggler(self.show_artists).on_toggle(Message::ToggleShowArtists),
            ))
            .add(settings::item(
                "Show artwork",
                toggler(self.show_artwork).on_toggle(Message::ToggleShowArtwork),
            ));

        self.core.applet.popup_container(content).into()
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Self::Message> {
        Some(Message::PopupClosed(id))
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(applet::style())
    }
}

fn shorten(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let shortened: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

async fn fetch_now_playing() -> Option<TrackInfo> {
    tokio::task::spawn_blocking(|| {
        let finder = PlayerFinder::new().ok()?;
        let mut paused_fallback: Option<TrackInfo> = None;

        // Try common players first, then fall back to all players.
        let players = [
            vec!["Spotify", "spotify"],
            vec!["SoundCloud", "soundcloud"],
            vec!["YouTube", "youtube", "YTMusic"],
            vec!["VLC", "vlc", "org.videolan.VLC"],
            vec!["mpd", "MPD"],
        ];

        // Prefer actively playing sessions over paused sessions.
        for names in &players {
            for name in names {
                if let Ok(player) = finder.find_by_name(name) {
                    if let Some((status, track)) = track_from_player(&player) {
                        if matches!(status, PlaybackStatus::Playing) {
                            return Some(track);
                        }
                        if paused_fallback.is_none() {
                            paused_fallback = Some(track);
                        }
                    }
                }
            }
        }

        // If none of the common names matched, scan all available MPRIS players.
        if let Ok(players) = finder.find_all() {
            for player in players {
                if let Some((status, track)) = track_from_player(&player) {
                    if matches!(status, PlaybackStatus::Playing) {
                        return Some(track);
                    }
                    if paused_fallback.is_none() {
                        paused_fallback = Some(track);
                    }
                }
            }
        }

        paused_fallback
    })
    .await
    .ok()
    .flatten()
}

fn track_from_player(player: &Player) -> Option<(PlaybackStatus, TrackInfo)> {
    let status = player.get_playback_status().ok()?;
    if matches!(status, PlaybackStatus::Stopped) {
        return None;
    }

    let metadata = player.get_metadata().ok()?;
    let title = metadata.title().map(str::trim).unwrap_or("");

    let artists = metadata
        .artists()
        .map(|v| v.join(", "))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "".to_string());

    Some((
        status,
        TrackInfo {
            title: title.to_string(),
            artists,
            art_url: metadata.art_url().map(str::to_string),
            media_url: metadata.url().map(str::to_string),
        },
    ))
}

fn art_cache_key(track: &TrackInfo) -> Option<String> {
    track
        .art_url
        .as_ref()
        .map(|url| format!("art:{url}"))
        .or_else(|| track.media_url.as_ref().map(|url| format!("media:{url}")))
}

async fn fetch_album_art_for_track(track: TrackInfo) -> Option<Vec<u8>> {
    let url = match track.art_url {
        Some(url) => Some(url),
        None => {
            let media_url = track.media_url?;
            resolve_thumbnail_url_from_media_url(&media_url).await
        }
    }?;

    fetch_album_art(url).await
}

async fn resolve_thumbnail_url_from_media_url(media_url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(media_url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();

    if host.contains("youtube.com") || host.contains("youtu.be") {
        // Try direct thumbnail first, then fallback to oEmbed
        if let Some(url) = extract_youtube_thumbnail(media_url).await {
            return Some(url);
        }
        return fetch_oembed_thumbnail("https://www.youtube.com/oembed", media_url).await;
    }

    if host.contains("soundcloud.com") || host.contains("snd.sc") {
        return fetch_oembed_thumbnail("https://soundcloud.com/oembed", media_url).await;
    }

    None
}

async fn extract_youtube_thumbnail(media_url: &str) -> Option<String> {
    // Extract video ID from YouTube URL
    let parsed = reqwest::Url::parse(media_url).ok()?;
    let video_id = if let Some(query_str) = parsed.query() {
        // Handle youtu.be short links (v parameter) or youtube.com with query
        query_str
            .split('&')
            .find(|s| s.starts_with("v="))
            .and_then(|s| s.strip_prefix("v="))
            .map(str::to_string)
    } else {
        // Try to extract from path for youtu.be links
        let path = parsed.path().trim_start_matches('/');
        if !path.is_empty() && path.len() < 50 {
            Some(path.to_string())
        } else {
            None
        }
    }?;

    // Clean up video ID (remove any fragments or extra params)
    let video_id = video_id.split('&').next().unwrap_or(&video_id).to_string();

    // Return direct thumbnail URL; maxresdefault is most common, fallbacks handled by fetch_album_art
    Some(format!(
        "https://img.youtube.com/vi/{}/maxresdefault.jpg",
        video_id
    ))
}

async fn fetch_oembed_thumbnail(endpoint: &str, media_url: &str) -> Option<String> {
    let url = reqwest::Url::parse_with_params(endpoint, &[("url", media_url), ("format", "json")])
        .ok()?;

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
        )
        .send()
        .await
        .ok()?;

    let payload: serde_json::Value = response.json().await.ok()?;

    // Try multiple possible field names
    payload
        .get("thumbnail_url")
        .or_else(|| payload.get("thumbnail"))
        .or_else(|| payload.get("image"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

async fn fetch_album_art(url: String) -> Option<Vec<u8>> {
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
        )
        .send()
        .await
        .ok()?;
    let bytes = response.bytes().await.ok()?;
    Some(bytes.to_vec())
}

fn config_file_path() -> Option<PathBuf> {
    let base_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".config"))
        })?;

    Some(base_dir.join(APP_ID).join(PREFS_FILE_NAME))
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn load_panel_visibility() -> Option<PanelVisibility> {
    let path = config_file_path()?;
    let content = fs::read_to_string(path).ok()?;

    let mut visibility = PanelVisibility::default();
    for line in content.lines() {
        let mut parts = line.splitn(2, '=');
        let key = parts.next()?.trim();
        let value = parts.next()?.trim();
        let parsed = parse_bool(value)?;

        match key {
            "show_title" => visibility.show_title = parsed,
            "show_artists" => visibility.show_artists = parsed,
            "show_artwork" => visibility.show_artwork = parsed,
            _ => {}
        }
    }

    Some(visibility)
}

fn save_panel_visibility(visibility: PanelVisibility) -> Option<()> {
    let path = config_file_path()?;
    let dir = path.parent()?;
    fs::create_dir_all(dir).ok()?;

    let content = format!(
        "show_title={}\nshow_artists={}\nshow_artwork={}\n",
        visibility.show_title, visibility.show_artists, visibility.show_artwork
    );

    fs::write(path, content).ok()?;
    Some(())
}

fn main() -> cosmic::iced::Result {
    applet::run::<SpotifyApplet>(())
}
