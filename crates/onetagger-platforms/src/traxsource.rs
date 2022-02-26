use std::error::Error;
use reqwest::blocking::Client;
use chrono::NaiveDate;
use scraper::{Html, Selector};
use onetagger_tagger::{Track, AudioFileInfo, TaggerConfig, AutotaggerSource, MatchingUtils, TrackNumber, AutotaggerSourceBuilder, PlatformInfo};

pub struct Traxsource {
    client: Client
}

impl Traxsource {
    // Create new instance
    pub fn new() -> Traxsource {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:85.0) Gecko/20100101 Firefox/85.0")
            .build()
            .unwrap();
        Traxsource {
            client
        }
    }

    pub fn search_tracks(&self, query: &str) -> Result<Vec<Track>, Box<dyn Error>> {
        // Fetch
        debug!("Q: {}", query);
        let data = self.client.get("https://www.traxsource.com/search/tracks")
            .query(&[("term", query)])
            .send()?
            .text()?;

        // Minify and parse
        let data = String::from_utf8(minify_html::minify(&data.as_bytes(), &minify_html::Cfg::spec_compliant()))?;
        let document = Html::parse_document(&data);

        // Track list
        let list_selector = Selector::parse("div#searchTrackList").unwrap();
        let track_list = document.select(&list_selector).next().ok_or("No results!")?;
        // Select track
        let track_selector = Selector::parse("div.trk-row").unwrap();
        let mut tracks = vec![];
        for track_element in track_list.select(&track_selector) {
            // Get title
            let mut selector = Selector::parse("div.title").unwrap();
            let title_elem = track_element.select(&selector).next().unwrap();
            let title_vec = title_elem.text().collect::<Vec<_>>();
            let title = title_vec[0].to_owned();
            let (version, duration) = match title_vec.len() {
                3 => {
                    // Remove space at end because of duration
                    let mut v = title_vec[1].to_owned();
                    v.pop();
                    // Parse duration
                    let duration = MatchingUtils::parse_duration(title_vec[2]).unwrap();
                    (Some(v), duration)
                },
                _ => (None, MatchingUtils::parse_duration(title_vec[1]).unwrap())
            };

            // Get URL, ID
            selector = Selector::parse("a").unwrap();
            let title_link = title_elem.select(&selector).next().unwrap();
            let title_href = title_link.value().attr("href").unwrap();
            let mut track_id = title_href.replace("/track/", "");
            track_id = track_id[..track_id.find("/").unwrap()].to_string();
            let url = format!("https://www.traxsource.com{}", title_href);

            // Artists
            selector = Selector::parse("div.artists a").unwrap();
            let artists: Vec<String> = track_element.select(&selector).map(|e| {
                e.text().collect::<Vec<_>>().first().unwrap().to_owned().to_owned()
            }).collect();
            
            // Label
            selector = Selector::parse("div.label").unwrap();
            let label = track_element.select(&selector).next().unwrap().text().collect::<Vec<_>>().first().unwrap().to_owned();
            
            // Key, BPM
            selector = Selector::parse("div.key-bpm").unwrap();
            let key_bpm_values = track_element.select(&selector).next().unwrap().text().collect::<Vec<_>>();
            let mut key = None;
            let mut bpm: Option<i64> = None;
            if key_bpm_values.len() == 2 {
                key = Some(key_bpm_values[0].to_owned()
                    .replace("maj", "")
                    .replace("min", "m"));
                bpm = key_bpm_values[1].to_owned().parse().ok();
            }
            
            // Genre
            selector = Selector::parse("div.genre").unwrap();
            let genre = track_element.select(&selector).next().unwrap().text().collect::<Vec<_>>().first().unwrap().to_owned();

            // Release date
            selector = Selector::parse("div.r-date").unwrap();
            let release_date_raw = track_element.select(&selector).next().unwrap().text().collect::<Vec<_>>().first().unwrap().to_owned();
            let release_date_clean = release_date_raw.replace("Pre-order for ", "");
            let release_date = release_date_clean.trim().to_owned();

            // Create track
            tracks.push(Track {
                platform: "traxsource".to_string(),
                version, artists, bpm, key, title, url,
                album_artists: vec![],
                label: Some(label.to_string()),
                release_date: NaiveDate::parse_from_str(&release_date, "%Y-%m-%d").ok(),
                genres: vec![genre.to_owned()],
                styles: vec![],
                other: vec![],
                track_id: Some(track_id),
                release_id: String::new(),
                duration,
                ..Default::default()
            })
        }

        Ok(tracks)
    }

    // Tracks in search don't have album name and art
    pub fn extend_track(&self, track: &mut Track, album_meta: bool) -> Result<(), Box<dyn Error>> {
        // Fetch
        let data = self.client.get(&track.url)
            .send()?
            .text()?;
        
        // Minify and parse
        let data = String::from_utf8(minify_html::minify(data.as_bytes(), &minify_html::Cfg::spec_compliant()))?;
        let document = Html::parse_document(&data);

        // Select album element
        let mut selector = Selector::parse("div.ttl-info.ellip a").unwrap();
        let album_element = document.select(&selector).next().unwrap();
        let album_url = album_element.value().attr("href").unwrap();
        let album_text = album_element.text().collect::<Vec<_>>();
        track.album = Some(album_text.first().unwrap().to_owned().to_owned());

        // Get release id
        let release_id = album_url.replace("/title/", "");
        track.release_id = release_id[..release_id.find("/").unwrap()].to_string();

        // Album metadata
        if !album_meta { 
            return Ok(());
        }
        let data = self.client.get(format!("https://www.traxsource.com{}", album_url))
            .send()?
            .text()?;
        // Minify and parse
        let data = String::from_utf8(minify_html::minify(data.as_bytes(), &minify_html::Cfg::spec_compliant()))?;
        let document = Html::parse_document(&data);

        // Select catalog number
        selector = Selector::parse("div.cat-rdate").unwrap();
        let rdate_element = document.select(&selector).next().unwrap();
        let release_date = rdate_element.text().collect::<Vec<_>>().join(" ");
        let rd_split = release_date.split(" | ").collect::<Vec<_>>();
        let mut catalog_number = None;
        if rd_split.len() >= 2 {
            catalog_number = Some(rd_split[0].trim().to_string());
        }
        track.catalog_number = catalog_number;

        // Album artists
        selector = Selector::parse("h1.artists").unwrap();
        let album_artists_element = document.select(&selector).next().unwrap();
        let album_artists_text = album_artists_element.text().collect::<Vec<_>>().join(" ");
        let album_artists: Vec<String> = album_artists_text.split(",").map(String::from).collect();
        track.album_artists = album_artists;

        // Track number
        selector = Selector::parse(&format!("div.trk-row.ptk-{}", track.track_id.as_ref().unwrap())).unwrap();
        let track_element = document.select(&selector).next().unwrap();
        selector = Selector::parse("div.tnum").unwrap();
        let track_number_element = track_element.select(&selector).next().unwrap();
        let track_number_text = track_number_element.text().collect::<Vec<_>>().join("");
        if let Ok(tn) = track_number_text.trim().parse() {
            track.track_number = Some(TrackNumber::Number(tn));
        }

        // Album art
        selector = Selector::parse("div.t-image img").unwrap();
        let img_element = document.select(&selector).next().unwrap();
        let art_url = img_element.value().attr("src").unwrap();
        track.art = Some(art_url.to_owned());

        Ok(())
    }
}

impl AutotaggerSource for Traxsource {
    fn match_track(&mut self, info: &AudioFileInfo, config: &TaggerConfig) -> Result<Option<(f64, Track)>, Box<dyn Error>> {
        // Search
        let query = format!("{} {}", info.artist()?, MatchingUtils::clean_title(info.title()?));
        let tracks = self.search_tracks(&query)?;
        // Match
        if let Some((acc, mut track)) = MatchingUtils::match_track(&info, &tracks, &config, true) {
            // Extend track if requested tags
            if config.album_art || config.album || config.catalog_number || config.release_id || config.album_artist || config.track_number {
                match self.extend_track(&mut track, config.catalog_number || config.track_number || config.album_art) {
                    Ok(_) => {},
                    Err(e) => warn!("Failed extending Traxsource track (album info might not be available): {}", e)
                }
            }
            return Ok(Some((acc, track)));
        }
        Ok(None)
    }
}

pub struct TraxsourceBuilder;

impl AutotaggerSourceBuilder for TraxsourceBuilder {
    fn new() -> TraxsourceBuilder {
        TraxsourceBuilder
    }

    fn get_source(&mut self, _config: &TaggerConfig) -> Result<Box<dyn AutotaggerSource>, Box<dyn Error>> {
        Ok(Box::new(Traxsource::new()))
    }

    fn info(&self) -> PlatformInfo {
        PlatformInfo {
            id: "traxsource".to_string(),
            name: "Traxsource".to_string(),
            description: "Overall more specialized in House".to_string(),
            icon: include_bytes!("../assets/traxsource.png"),
            max_threads: 0,
            version: "1.0.0".to_string(),
            custom_options: Default::default(),
        }
    }
}