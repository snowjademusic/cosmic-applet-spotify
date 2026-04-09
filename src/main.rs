use cosmic::app::{Core, Task};
use cosmic::iced::{
    self,
    platform_specific::shell::commands::popup,
    widget::{container, image as iced_image, row, text},
    window, Alignment, Length,
};
use cosmic::{applet, executor, Element};
use cosmic::widget::{list_column, settings, toggler};
use mpris::{PlaybackStatus, PlayerFinder};
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
                    && (art_url.as_deref() != self.art_url_loaded.as_deref()
                        || self.album_art.is_none());
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

        let art: Option<Element<'_, Message>> = if self.show_artwork {
            Some(if let Some(bytes) = &self.album_art {
                let handle = iced_image::Handle::from_bytes(bytes.as_ref().clone());
                iced_image::Image::new(handle)
                    .width(Length::Fixed(icon_size))
                    .height(Length::Fixed(icon_size))
                    .into()
            } else {
                container(text("♫"))
                    .width(Length::Fixed(icon_size))
                    .height(Length::Fixed(icon_size))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .into()
            })
        } else {
            None
        };

        let title_part = if self.show_title {
            self.track
                .as_ref()
                .map(|t| shorten(&t.title, 24))
                .unwrap_or_else(|| String::from("Spotify"))
        } else {
            String::new()
        };

        let artist_part = if self.show_artists {
            self.track
                .as_ref()
                .map(|t| shorten(&t.artists, 24))
                .unwrap_or_else(|| String::from("No artist"))
        } else {
            String::new()
        };

        let label = match (self.show_title, self.show_artists) {
            (true, true) => format!("{} • {}", title_part, artist_part),
            (true, false) => title_part,
            (false, true) => artist_part,
            (false, false) => String::from("Spotify"),
        };

        let mut panel_row = row![]
            .spacing(8)
            .align_y(Alignment::Center);
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
                        .class(cosmic::theme::Button::AppletIcon)
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
        let player = finder
            .find_by_name("spotify")
            .or_else(|_| finder.find_by_name("Spotify"))
            .ok()?;

        let status = player.get_playback_status().ok()?;
        if matches!(status, PlaybackStatus::Stopped) {
            return None;
        }

        let metadata = player.get_metadata().ok()?;
        Some(TrackInfo {
            title: metadata.title().unwrap_or("Unknown title").to_string(),
            artists: metadata
                .artists()
                .map(|v| v.join(", "))
                .unwrap_or_else(|| "Unknown artist".to_string()),
            art_url: metadata.art_url().map(str::to_string),
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
        visibility.show_title,
        visibility.show_artists,
        visibility.show_artwork
    );

    fs::write(path, content).ok()?;
    Some(())
}

fn main() -> cosmic::iced::Result {
    applet::run::<SpotifyApplet>(())
}