use cosmic::app::{Core, Task};
use cosmic::iced::{
    self,
    platform_specific::shell::commands::popup,
    widget::{column, container, image as iced_image, row, text},
    window, Alignment, Length,
};
use cosmic::{applet, executor, Element};
use mpris::{PlaybackStatus, PlayerFinder};
use std::sync::Arc;

const APP_ID: &str = "com.example.CosmicAppletSpotify";
const POLL_INTERVAL_SECONDS: u64 = 3;

// ── We map PlaybackStatus to a String immediately so TrackInfo stays Clone ──
#[derive(Clone, Debug)]
struct TrackInfo {
    title: String,
    artists: String,
    art_url: Option<String>,
    status: String, // "Playing" | "Paused" | "Stopped"
}

#[derive(Debug, Clone)]
enum Message {
    TogglePopup,
    PopupClosed(window::Id),
    RefreshNowPlaying,
    NowPlayingLoaded(Option<TrackInfo>),
    AlbumArtFetched(Option<Arc<Vec<u8>>>, String),
}

struct SpotifyApplet {
    core: Core,
    popup: Option<window::Id>,
    track: Option<TrackInfo>,
    album_art: Option<Arc<Vec<u8>>>,
    art_url_loaded: Option<String>,
}

impl Default for SpotifyApplet {
    fn default() -> Self {
        Self {
            core: Core::default(),
            popup: None,
            track: None,
            album_art: None,
            art_url_loaded: None,
        }
    }
}

impl cosmic::Application for SpotifyApplet {
    type Executor = executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core { &self.core }
    fn core_mut(&mut self) -> &mut Core { &mut self.core }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        (
            Self { core, ..Default::default() },
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

            Message::RefreshNowPlaying => {
                Task::perform(fetch_now_playing(), |track| {
                    cosmic::Action::App(Message::NowPlayingLoaded(track))
                })
            }

            Message::NowPlayingLoaded(track) => {
                let art_url = track.as_ref().and_then(|t| t.art_url.clone());
                let needs_art = art_url.is_some()
                    && art_url.as_deref() != self.art_url_loaded.as_deref();
                self.track = track;
                if needs_art {
                    let url = art_url.unwrap();
                    Task::perform(fetch_album_art(url.clone()), move |bytes| {
                        cosmic::Action::App(Message::AlbumArtFetched(
                            bytes.map(Arc::new),
                            url.clone(),
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
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let label = match &self.track {
            Some(t) => format!("♪ {}", shorten(&t.title, 24)),
            None => String::from("♪ Spotify"),
        };

        self.core
            .applet
            .autosize_window(
                container(
                    self.core.applet.text_button(text(label), Message::TogglePopup)
                )
                .padding([0, 10]),
            )
            .into()
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Self::Message> {
        let content: Element<'_, Message> = match &self.track {
            None => text("Nothing playing").into(),
            Some(track) => {
                let art: Element<'_, Message> = if let Some(bytes) = &self.album_art {
                    let handle = iced_image::Handle::from_bytes(bytes.as_ref().clone());
                    iced_image::Image::new(handle)
                        .width(Length::Fixed(72.0))
                        .height(Length::Fixed(72.0))
                        .into()
                } else {
                    container(text("♫"))
                        .width(Length::Fixed(72.0))
                        .height(Length::Fixed(72.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                        .into()
                };

                row![
                    art,
                    column![
                        text(&track.title).size(15),
                        text(&track.artists).size(12),
                        text(&track.status).size(11),
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
        let player = finder
            .find_by_name("spotify")
            .or_else(|_| finder.find_by_name("Spotify"))
            .ok()?;

        let status = player.get_playback_status().ok()?;
        if matches!(status, PlaybackStatus::Stopped) {
            return None;
        }

        let status_str = match status {
            PlaybackStatus::Playing => "Playing",
            PlaybackStatus::Paused  => "Paused",
            PlaybackStatus::Stopped => "Stopped",
        }.to_string();

        let metadata = player.get_metadata().ok()?;
        Some(TrackInfo {
            title: metadata.title().unwrap_or("Unknown title").to_string(),
            artists: metadata
                .artists()
                .map(|v| v.join(", "))
                .unwrap_or_else(|| "Unknown artist".to_string()),
            art_url: metadata.art_url().map(str::to_string),
            status: status_str,
        })
    })
    .await
    .ok()
    .flatten()
}

async fn fetch_album_art(url: String) -> Option<Vec<u8>> {
    let response = reqwest::get(&url).await.ok()?;
    let bytes = response.bytes().await.ok()?;
    Some(bytes.to_vec())
}

fn main() -> cosmic::iced::Result {
    applet::run::<SpotifyApplet>(())
}