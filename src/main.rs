use cosmic::{
    app::{Core, Task},
    applet,
    iced::{
        self,
        widget::{column, container, image as iced_image, row, text},
        window, Alignment, Length,
    },
    Element,
};
use mpris::PlayerFinder;
use std::sync::Arc;
use tokio::sync::Mutex;

const APP_ID: &str = "com.example.CosmicAppletSpotify";
const POLL_INTERVAL_MS: u64 = 2000;

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct TrackInfo {
    title: String,
    artist: String,
    art_url: Option<String>,
}

struct SpotifyApplet {
    core: Core,
    popup: Option<window::Id>,
    track: Option<TrackInfo>,
    album_art: Option<Arc<Vec<u8>>>, // raw image bytes for iced
    art_url_loaded: Option<String>,   // track which URL is loaded
}

#[derive(Debug, Clone)]
enum Message {
    TogglePopup,
    TrackChanged(Option<TrackInfo>),
    AlbumArtFetched(Option<Arc<Vec<u8>>>, String), // bytes + url that was fetched
    Tick,
}

// ── App impl ─────────────────────────────────────────────────────────────────

impl cosmic::Application for SpotifyApplet {
    type Message = Message;
    type Executor = cosmic::executor::Default; // tokio under the hood
    type Flags = ();
    const APP_ID: &'static str = APP_ID;

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
        let applet = Self {
            core,
            popup: None,
            track: None,
            album_art: None,
            art_url_loaded: None,
        };
        // Kick off first poll immediately
        (applet, Task::perform(poll_mpris(), Message::TrackChanged))
    }

    fn core(&self) -> &Core { &self.core }
    fn core_mut(&mut self) -> &mut Core { &mut self.core }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TogglePopup => {
                if let Some(p) = self.popup.take() {
                    return applet::commands::popup::destroy_popup(p);
                }
                let new_id = window::Id::unique();
                self.popup = Some(new_id);
                let settings = self.core.applet.get_popup_settings(
                    self.core.main_window_id().unwrap(),
                    new_id,
                    Some((360, 120)),
                    None,
                    None,
                );
                applet::commands::popup::get_popup(settings)
            }

            Message::TrackChanged(info) => {
                // Check if track changed → need new art
                let art_url = info.as_ref().and_then(|t| t.art_url.clone());
                let needs_fetch = art_url.is_some()
                    && art_url.as_deref() != self.art_url_loaded.as_deref();

                self.track = info;

                let fetch_task = if needs_fetch {
                    let url = art_url.clone().unwrap();
                    Task::perform(fetch_album_art(url.clone()), move |bytes| {
                        Message::AlbumArtFetched(bytes.map(Arc::new), url.clone())
                    })
                } else {
                    Task::none()
                };

                // Schedule next poll
                let poll_task = Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
                        poll_mpris().await
                    },
                    Message::TrackChanged,
                );

                Task::batch([fetch_task, poll_task])
            }

            Message::AlbumArtFetched(bytes, url) => {
                self.album_art = bytes;
                self.art_url_loaded = Some(url);
                Task::none()
            }

            Message::Tick => Task::perform(poll_mpris(), Message::TrackChanged),
        }
    }

    // Panel button: show truncated track name + music icon
    fn view(&self) -> Element<Message> {
        let label = match &self.track {
            Some(t) => {
                let display = if t.title.len() > 25 {
                    format!("{}…", &t.title[..24])
                } else {
                    t.title.clone()
                };
                format!("♪ {}", display)
            }
            None => "♪".to_string(),
        };

        self.core
            .applet
            .autosize_window(
                container(
                    self.core
                        .applet
                        .text_button(&label)
                        .on_press(Message::TogglePopup),
                )
                .padding([0, 8]),
                None,
            )
            .into()
    }

    // Popup: album art + full track info
    fn view_window(&self, _id: window::Id) -> Element<Message> {
        let content: Element<Message> = match &self.track {
            None => text("Nothing playing").into(),
            Some(track) => {
                let art: Element<Message> = if let Some(bytes) = &self.album_art {
                    let handle = iced_image::Handle::from_bytes(bytes.as_ref().clone());
                    iced_image::Image::new(handle)
                        .width(Length::Fixed(64.0))
                        .height(Length::Fixed(64.0))
                        .into()
                } else {
                    container(text("🎵"))
                        .width(Length::Fixed(64.0))
                        .height(Length::Fixed(64.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                        .into()
                };

                row![
                    art,
                    column![
                        text(&track.title).size(14),
                        text(&track.artist).size(12),
                    ]
                    .spacing(4)
                    .align_x(Alignment::Start),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .into()
            }
        };

        self.core.applet.popup_container(content).into()
    }
}

// ── MPRIS polling (runs in tokio via spawn_blocking) ─────────────────────────

async fn poll_mpris() -> Option<TrackInfo> {
    tokio::task::spawn_blocking(|| {
        let finder = PlayerFinder::new().ok()?;

        // Find Spotify specifically; fall back to any active player
        let player = finder
            .find_by_name("Spotify")
            .or_else(|_| finder.find_active())
            .ok()?;

        let meta = player.get_metadata().ok()?;
        let status = player.get_playback_status().ok()?;

        if status != mpris::PlaybackStatus::Playing {
            return None; // only show when actually playing
        }

        let meta = player.get_metadata().ok()?;
        Some(TrackInfo {
            title: meta.title().unwrap_or("Unknown").to_string(),
            artist: meta
                .artists()
                .and_then(|v| v.first().map(|s| s.to_string()))
                .unwrap_or_else(|| "Unknown".to_string()),
            art_url: meta.art_url().map(str::to_string),
        })
    })
    .await
    .ok()
    .flatten()
}

// ── Album art fetch ──────────────────────────────────────────────────────────

async fn fetch_album_art(url: String) -> Option<Vec<u8>> {
    // Spotify's art_url is an https:// URL to their CDN
    let response = reqwest::get(&url).await.ok()?;
    let bytes = response.bytes().await.ok()?;
    Some(bytes.to_vec())
}

// ── Entry point ──────────────────────────────────────────────────────────────

fn main() -> cosmic::iced::Result {
    applet::run::<SpotifyApplet>(())
}