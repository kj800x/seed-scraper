use anyhow::{Context, Result};
use chrono::{Days, NaiveDate};
use clap::Parser;
use regex;
use scraper::Element;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, thread, time::Duration as StdDuration};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Parser)]
enum Commands {
    /// Scrape a single URL
    Single {
        #[arg(short, long)]
        url: String,
    },
    /// Process a CSV file containing plant names and URLs
    Batch {
        #[arg(short, long)]
        file: String,
    },
    /// Export data from JSON files to CSV, using input CSV for additional columns
    Export {
        #[arg(short, long)]
        file: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct PlantInfo {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    days_to_maturity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plant_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    native: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hardiness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exposure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plant_dimensions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variety_info: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attributes: Option<String>,
    // Sowing Info
    #[serde(skip_serializing_if = "Option::is_none")]
    when_to_sow_outside: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    when_to_start_inside: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    days_to_emerge: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed_depth: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed_spacing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    row_spacing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinning: Option<String>,
    // Rating Info
    #[serde(skip_serializing_if = "Option::is_none")]
    rating: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    votes: Option<u32>,
}

#[derive(Debug)]
enum ScrapingError {
    CloudflareBlocked,
    #[allow(dead_code)]
    Other(anyhow::Error),
}

impl std::fmt::Display for ScrapingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScrapingError::CloudflareBlocked => write!(
                f,
                "Access blocked by Cloudflare. Try again later or check if the URL is correct."
            ),
            ScrapingError::Other(e) => write!(f, "Error scraping page: {}", e),
        }
    }
}

impl std::error::Error for ScrapingError {}

impl PlantInfo {
    fn normalize_text(text: &str) -> String {
        text.replace('\u{2013}', "-").replace('\u{2014}', "-")
    }

    fn from_html(html: &str, url: String) -> Result<Self, ScrapingError> {
        if html.contains("Attention Required! | Cloudflare")
            || html.contains("Sorry, you have been blocked")
            || html.contains("Please enable cookies.")
        {
            return Err(ScrapingError::CloudflareBlocked);
        }

        let document = Html::parse_document(html);
        let info_selector = Selector::parse("div.tab-content p b").unwrap();
        let rating_selector = Selector::parse("div.loox-rating").unwrap();
        let title_selector = Selector::parse("h1").unwrap();
        let description_selector = Selector::parse(".product__description").unwrap();

        let mut info = PlantInfo {
            url,
            title: None,
            description: None,
            days_to_maturity: None,
            family: None,
            plant_type: None,
            native: None,
            hardiness: None,
            exposure: None,
            plant_dimensions: None,
            variety_info: None,
            attributes: None,
            when_to_sow_outside: None,
            when_to_start_inside: None,
            days_to_emerge: None,
            seed_depth: None,
            seed_spacing: None,
            row_spacing: None,
            thinning: None,
            rating: None,
            votes: None,
        };

        // Parse title
        if let Some(title_element) = document.select(&title_selector).next() {
            info.title = Some(Self::normalize_text(
                &title_element.text().collect::<String>(),
            ));
        }

        // Parse description
        if let Some(desc_element) = document.select(&description_selector).next() {
            info.description = Some(Self::normalize_text(
                &desc_element.text().collect::<String>().trim(),
            ));
        }

        // Parse rating information
        if let Some(rating_element) = document.select(&rating_selector).next() {
            if let (Some(rating), Some(votes)) = (
                rating_element.value().attr("data-rating"),
                rating_element.value().attr("data-raters"),
            ) {
                info.rating = rating.parse().ok();
                info.votes = votes.parse().ok();
            }
        }

        for element in document.select(&info_selector) {
            let label = element.text().collect::<Vec<_>>().join("");
            if let Some(parent) = element.parent_element() {
                let full_text = parent.text().collect::<Vec<_>>().join("");
                let normalized = Self::normalize_text(&full_text.replace(&label, "").trim());
                match label.trim_end_matches(':') {
                    "Days to Maturity" => info.days_to_maturity = Some(normalized),
                    "Family" => info.family = Some(normalized),
                    "Type" => info.plant_type = Some(normalized.replace(" (Learn more)", "")),
                    "Native" => info.native = Some(normalized),
                    "Hardiness" => info.hardiness = Some(normalized),
                    "Exposure" => info.exposure = Some(normalized),
                    "Plant Dimensions" => info.plant_dimensions = Some(normalized),
                    "Variety Info" => info.variety_info = Some(normalized),
                    "Attributes" => info.attributes = Some(normalized),
                    "When to Sow Outside" => info.when_to_sow_outside = Some(normalized),
                    "When to Start Inside" => info.when_to_start_inside = Some(normalized),
                    "Days to Emerge" => info.days_to_emerge = Some(normalized),
                    "Seed Depth" => info.seed_depth = Some(normalized),
                    "Seed Spacing" => info.seed_spacing = Some(normalized),
                    "Row Spacing" => info.row_spacing = Some(normalized),
                    "Thinning" => info.thinning = Some(normalized),
                    _ => (),
                }
            }
        }

        Ok(info)
    }
}

fn process_csv(file_path: &str) -> Result<()> {
    let results_dir = Path::new("results");
    if !results_dir.exists() {
        fs::create_dir(results_dir).context("Failed to create results directory")?;
    }

    let mut failed_plants = Vec::new();
    let mut rdr = csv::Reader::from_path(file_path).context("Failed to read CSV file")?;

    for result in rdr.records() {
        let record = match result {
            Ok(record) => record,
            Err(e) => {
                eprintln!("Error reading CSV record: {}", e);
                continue;
            }
        };

        let plant = record.get(0).unwrap_or("unknown");
        let url = match record.get(1) {
            Some(url) => url,
            None => {
                eprintln!("Missing URL for plant: {}", plant);
                failed_plants.push(plant.to_string());
                continue;
            }
        };

        let file_name = format!("results/{}.json", plant.replace("/", "_"));

        // Skip if file already exists
        if Path::new(&file_name).exists() {
            println!("Skipping {} - result file already exists", plant);
            continue;
        }

        println!("Processing {} from {}", plant, url);

        // Sleep between requests
        thread::sleep(StdDuration::from_secs(2));

        let client = reqwest::blocking::Client::new();
        let response = match client
            .get(url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.5")
            .header("Connection", "keep-alive")
            .send()
            .and_then(|r| r.text()) {
                Ok(text) => text,
                Err(e) => {
                    eprintln!("Failed to fetch {}: {}", plant, e);
                    failed_plants.push(plant.to_string());
                    continue;
                }
            };

        match PlantInfo::from_html(&response, url.to_string()) {
            Ok(info) => {
                let json = match serde_json::to_string_pretty(&info) {
                    Ok(j) => j,
                    Err(e) => {
                        eprintln!("Failed to serialize JSON for {}: {}", plant, e);
                        failed_plants.push(plant.to_string());
                        continue;
                    }
                };

                if let Err(e) = fs::write(&file_name, json) {
                    eprintln!("Failed to write file for {}: {}", plant, e);
                    failed_plants.push(plant.to_string());
                }
            }
            Err(e) => {
                eprintln!("Failed to parse HTML for {}: {}", plant, e);
                failed_plants.push(plant.to_string());
            }
        }
    }

    if !failed_plants.is_empty() {
        eprintln!("\nFailed to process the following plants:");
        for plant in &failed_plants {
            eprintln!("- {}", plant);
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
enum TimingType {
    LastFrost,
    Transplant,
}

#[derive(Debug, Clone)]
struct SowingTime {
    weeks_min: i64,
    weeks_max: i64,
    relative_timing: RelativeTiming,
    timing_type: TimingType,
}

#[derive(Debug, Clone)]
enum RelativeTiming {
    Before,
    After,
}

fn extract_weeks_pattern(text: &str) -> Option<SowingTime> {
    let re = regex::Regex::new(
        r"(\d+)\s*to\s*(\d+)\s*weeks\s*(before|after)\s*(your average last frost date|transplanting)",
    )
    .unwrap();

    re.captures(text).and_then(|cap| {
        let timing_type = match cap.get(4).unwrap().as_str() {
            "your average last frost date" => TimingType::LastFrost,
            "transplanting" => TimingType::Transplant,
            _ => return None,
        };

        let relative_timing = match cap.get(3).unwrap().as_str() {
            "before" => RelativeTiming::Before,
            "after" => RelativeTiming::After,
            _ => unreachable!(),
        };

        Some(SowingTime {
            weeks_min: cap.get(1).unwrap().as_str().parse().unwrap(),
            weeks_max: cap.get(2).unwrap().as_str().parse().unwrap(),
            relative_timing,
            timing_type,
        })
    })
}

fn determine_sowing_strategy(info: &PlantInfo) -> Option<&'static str> {
    let outside = info.when_to_sow_outside.as_deref();
    let inside = info.when_to_start_inside.as_deref();

    match (outside, inside) {
        (Some(out), _) if out.contains("RECOMMENDED") => Some("Outside"),
        (_, Some(ins)) if ins.contains("RECOMMENDED") => Some("Inside"),
        (Some(_), None) => Some("Outside"), // Default to outside if no inside instructions
        (None, Some(_)) => Some("Inside"),  // Default to inside if no outside instructions
        (Some(_), Some(_)) => Some("Outside"), // Default to outside if both are available but no recommendation
        _ => None,
    }
}

fn get_when_to_seed_start(info: &PlantInfo) -> Option<SowingTime> {
    let strategy = determine_sowing_strategy(info);
    let text = match strategy {
        Some("Inside") => info.when_to_start_inside.as_deref(),
        Some("Outside") => info.when_to_sow_outside.as_deref(),
        _ => None,
    };

    text.and_then(extract_weeks_pattern)
}

fn calculate_start_date(sowing_time: &SowingTime, frost_date: NaiveDate) -> NaiveDate {
    let base_date = match sowing_time.timing_type {
        TimingType::LastFrost => frost_date,
        TimingType::Transplant => frost_date + Days::new(21), // 3 weeks after frost date
    };

    match sowing_time.relative_timing {
        RelativeTiming::Before => base_date - Days::new((sowing_time.weeks_min * 7) as u64),
        RelativeTiming::After => base_date + Days::new((sowing_time.weeks_min * 7) as u64),
    }
}

fn export_to_csv(file_path: &str) -> Result<()> {
    let results_dir = Path::new("results");
    if !results_dir.exists() {
        return Err(anyhow::anyhow!("Results directory does not exist"));
    }

    // Read the input CSV file
    let mut input_rdr = csv::Reader::from_path(file_path)
        .context(format!("Failed to read input CSV file: {}", file_path))?;

    let mut writer = csv::Writer::from_path("export.csv")?;

    // Write headers - include the original columns plus the scraped data
    writer.write_record(&[
        "Plant Name",
        "URL",
        "Brand",         // New column
        "Purchase Year", // New column
        "Notes",         // New column
        "Title",
        "Description",
        "Days to Maturity",
        "Family",
        "Plant Type",
        "Native",
        "Hardiness",
        "Exposure",
        "Plant Dimensions",
        "Variety Info",
        "Attributes",
        "When to Sow Outside",
        "When to Start Inside",
        "Days to Emerge",
        "Seed Depth",
        "Seed Spacing",
        "Row Spacing",
        "Thinning",
        "Rating",
        "Votes",
        "Sowing Strategy",
        "When to Seed Start",
        "Calculated Start Date",
    ])?;

    let frost_date = NaiveDate::from_ymd_opt(2025, 5, 10).unwrap();

    // Process each row in the input CSV
    for result in input_rdr.records() {
        let record = match result {
            Ok(record) => record,
            Err(e) => {
                eprintln!("Error reading CSV record: {}", e);
                continue;
            }
        };

        // Get values from the input CSV
        let plant = record.get(0).unwrap_or("unknown");
        let url = record.get(1).unwrap_or("");
        let brand = record.get(2).unwrap_or("");
        let purchase_year = record.get(3).unwrap_or("");
        let notes = record.get(4).unwrap_or("");

        // Load the JSON file for this plant
        let json_path = format!("results/{}.json", plant.replace("/", "_"));

        if !Path::new(&json_path).exists() {
            eprintln!("Warning: No JSON data found for plant: {}", plant);
            // Write a row with just the input data and NULL for everything else
            let nulls: Vec<&str> = vec!["NULL"; 24]; // 24 columns of scraped data
            let mut row = vec![plant, url, brand, purchase_year, notes];
            row.extend(nulls);
            writer.write_record(&row)?;
            continue;
        }

        // Read and parse the JSON file
        let content = match fs::read_to_string(&json_path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Failed to read JSON file for {}: {}", plant, e);
                continue;
            }
        };

        let info: PlantInfo = match serde_json::from_str(&content) {
            Ok(info) => info,
            Err(e) => {
                eprintln!("Failed to parse JSON for {}: {}", plant, e);
                continue;
            }
        };

        let sowing_strategy = determine_sowing_strategy(&info)
            .map(String::from)
            .unwrap_or_else(|| "NULL".to_string());

        let when_to_start = get_when_to_seed_start(&info);

        let when_to_start_str = when_to_start
            .as_ref()
            .map(|sowing_time| {
                let relative = match sowing_time.relative_timing {
                    RelativeTiming::Before => "before",
                    RelativeTiming::After => "after",
                };
                let timing = match sowing_time.timing_type {
                    TimingType::LastFrost => "LAST_FROST",
                    TimingType::Transplant => "TRANSPLANT",
                };
                format!(
                    "{}-{} {} {}",
                    sowing_time.weeks_min, sowing_time.weeks_max, relative, timing
                )
            })
            .unwrap_or_else(|| "NULL".to_string());

        let start_date = when_to_start
            .map(|t| calculate_start_date(&t, frost_date))
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "NULL".to_string());

        let title = info.title.as_deref().unwrap_or("NULL");
        let description = info.description.as_deref().unwrap_or("NULL");
        let days_to_maturity = info.days_to_maturity.as_deref().unwrap_or("NULL");
        let family = info.family.as_deref().unwrap_or("NULL");
        let plant_type = info.plant_type.as_deref().unwrap_or("NULL");
        let native = info.native.as_deref().unwrap_or("NULL");
        let hardiness = info.hardiness.as_deref().unwrap_or("NULL");
        let exposure = info.exposure.as_deref().unwrap_or("NULL");
        let plant_dimensions = info.plant_dimensions.as_deref().unwrap_or("NULL");
        let variety_info = info.variety_info.as_deref().unwrap_or("NULL");
        let attributes = info.attributes.as_deref().unwrap_or("NULL");
        let when_to_sow_outside = info.when_to_sow_outside.as_deref().unwrap_or("NULL");
        let when_to_start_inside = info.when_to_start_inside.as_deref().unwrap_or("NULL");
        let days_to_emerge = info.days_to_emerge.as_deref().unwrap_or("NULL");
        let seed_depth = info.seed_depth.as_deref().unwrap_or("NULL");
        let seed_spacing = info.seed_spacing.as_deref().unwrap_or("NULL");
        let row_spacing = info.row_spacing.as_deref().unwrap_or("NULL");
        let thinning = info.thinning.as_deref().unwrap_or("NULL");

        // Create owned strings for these values to avoid referencing temporary values
        let rating_str = info.rating.map_or("NULL".to_string(), |r| r.to_string());
        let votes_str = info.votes.map_or("NULL".to_string(), |v| v.to_string());
        let rating = rating_str.as_str();
        let votes = votes_str.as_str();

        writer.write_record(&[
            plant,
            url,
            brand,
            purchase_year,
            notes,
            title,
            description,
            days_to_maturity,
            family,
            plant_type,
            native,
            hardiness,
            exposure,
            plant_dimensions,
            variety_info,
            attributes,
            when_to_sow_outside,
            when_to_start_inside,
            days_to_emerge,
            seed_depth,
            seed_spacing,
            row_spacing,
            thinning,
            rating,
            votes,
            &sowing_strategy,
            &when_to_start_str,
            &start_date,
        ])?;
    }

    writer.flush()?;
    println!("Exported data to export.csv");
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Single { url } => {
            // Existing single URL processing code
            let client = reqwest::blocking::Client::new();
            let response = client
                .get(&url)
                .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
                .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8")
                .header("Accept-Language", "en-US,en;q=0.5")
                .header("Connection", "keep-alive")
                .send()
                .context("Failed to send request")?
                .text()
                .context("Failed to get response text")?;

            match PlantInfo::from_html(&response, url) {
                Ok(info) => {
                    println!("{}", serde_json::to_string_pretty(&info)?);
                }
                Err(ScrapingError::CloudflareBlocked) => {
                    eprintln!("Error: Access blocked by Cloudflare protection");
                    eprintln!("Try again later or verify the URL is correct");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Batch { file } => {
            process_csv(&file)?;
        }
        Commands::Export { file } => {
            export_to_csv(&file)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_parse_plant_info() {
        let html = r#"
        <div class="tab-content">
            <div id="variety" data-tab-content class="active">
                <h3>Variety Info</h3>
                <p><b>Days to Maturity:</b> 65 days</p>
                <p><b>Family:</b> Apiaceae</p>
                <p><b>Type:</b> Danvers Type</p>
                <p><b>Native:</b> Africa, Eurasia</p>
                <p><b>Hardiness:</b> Frost-tolerant biennial grown as an annual</p>
                <p><b>Exposure:</b> Full sun</p>
                <p><b>Plant Dimensions:</b> Roots are 6"–7" long at their peak.</p>
                <p><b>Variety Info:</b> Orange roots, wide at the top, tapering to a point.</p>
                <p><b>Attributes:</b> Crack Resistant, Frost Tolerant</p>
            </div>
            <div id="sowing" data-tab-content>
                <h3>Sowing Info</h3>
                <p><b>When to Sow Outside:</b> RECOMMENDED. 2 to 4 weeks before your average last frost date</p>
                <p><b>When to Start Inside:</b> Not recommended; root disturbance stunts growth.</p>
                <p><b>Days to Emerge:</b> 10–25 days</p>
                <p><b>Seed Depth:</b> ¼"</p>
                <p><b>Seed Spacing:</b> 1"</p>
                <p><b>Row Spacing:</b> 6"</p>
                <p><b>Thinning:</b> When 1" tall, thin to 1 every 3"</p>
            </div>
        </div>
        "#;

        let info = PlantInfo::from_html(html, "http://example.com".to_string()).unwrap();

        assert_eq!(info.days_to_maturity.as_deref(), Some("65 days"));
        assert_eq!(info.family.as_deref(), Some("Apiaceae"));
        assert_eq!(info.plant_type.as_deref(), Some("Danvers Type"));
        assert_eq!(info.native.as_deref(), Some("Africa, Eurasia"));
        assert_eq!(
            info.hardiness.as_deref(),
            Some("Frost-tolerant biennial grown as an annual")
        );
        assert_eq!(info.exposure.as_deref(), Some("Full sun"));
        assert_eq!(
            info.plant_dimensions.as_deref(),
            Some("Roots are 6\"-7\" long at their peak.")
        );
        assert_eq!(
            info.variety_info.as_deref(),
            Some("Orange roots, wide at the top, tapering to a point.")
        );
        assert_eq!(
            info.attributes.as_deref(),
            Some("Crack Resistant, Frost Tolerant")
        );
        assert_eq!(
            info.when_to_sow_outside.as_deref(),
            Some("RECOMMENDED. 2 to 4 weeks before your average last frost date")
        );
        assert_eq!(
            info.when_to_start_inside.as_deref(),
            Some("Not recommended; root disturbance stunts growth.")
        );
        assert_eq!(info.days_to_emerge.as_deref(), Some("10-25 days"));
        assert_eq!(info.seed_depth.as_deref(), Some("¼\""));
        assert_eq!(info.seed_spacing.as_deref(), Some("1\""));
        assert_eq!(info.row_spacing.as_deref(), Some("6\""));
        assert_eq!(
            info.thinning.as_deref(),
            Some("When 1\" tall, thin to 1 every 3\"")
        );
    }

    #[test]
    fn test_parse_from_file() {
        let html = include_str!("../tests/fixtures/seed.html");
        let info = PlantInfo::from_html(html, "http://example.com".to_string()).unwrap();

        assert_eq!(info.title.as_deref(), Some("Danvers 126 Carrot Seeds"));
        assert_eq!(
            info.description.as_deref(),
            Some("Growers in Danvers, Massachusetts during the late-19th century were searching for a carrot with improved color, yield, and uniformity. After many variations, the 'Danvers 126' carrot was born! It grows particularly well interplanted with onions and in heavy soils due to its high fiber content. Heat-tolerant with high yields, it also has a noticeably sweeter flavor and stores exceptionally well if cleaned after harvest. Resistant to cracks and splits.")
        );
        assert_eq!(info.days_to_maturity.as_deref(), Some("65 days"));
        assert_eq!(info.family.as_deref(), Some("Apiaceae"));
        assert_eq!(info.plant_type.as_deref(), Some("Danvers Type"));
        assert_eq!(info.native.as_deref(), Some("Africa, Eurasia"));
        assert_eq!(
            info.hardiness.as_deref(),
            Some("Frost-tolerant biennial grown as an annual")
        );
        assert_eq!(info.exposure.as_deref(), Some("Full sun"));
        assert_eq!(
            info.plant_dimensions.as_deref(),
            Some("Roots are 6\"-7\" long at their peak.")
        );
        assert_eq!(info.variety_info.as_deref(), Some("Orange roots, wide at the top, tapering to a point. 'Danvers 126' is a Danvers type carrot."));
        assert_eq!(
            info.attributes.as_deref(),
            Some("Crack Resistant, Frost Tolerant")
        );
        assert_eq!(
            info.when_to_sow_outside.as_deref(),
            Some("RECOMMENDED. 2 to 4 weeks before your average last frost date, and when soil temperature is at least 45°F, ideally 60°-85°F. Successive Sowings: Every 3 weeks until 10 to 12 weeks before your average first fall frost date. In very warm climates, carrots are grown primarily in fall, winter, and spring.")
        );
        assert_eq!(
            info.when_to_start_inside.as_deref(),
            Some("Not recommended; root disturbance stunts growth.")
        );
        assert_eq!(info.days_to_emerge.as_deref(), Some("10-25 days"));
        assert_eq!(info.seed_depth.as_deref(), Some("¼\""));
        assert_eq!(info.seed_spacing.as_deref(), Some("1\""));
        assert_eq!(info.row_spacing.as_deref(), Some("6\""));
        assert_eq!(
            info.thinning.as_deref(),
            Some("When 1\" tall, thin to 1 every 3\"")
        );
        assert_eq!(info.rating, Some(4.5));
        assert_eq!(info.votes, Some(32));
    }

    #[test]
    fn test_extract_weeks_pattern() {
        // Test before last frost
        let text = "2 to 4 weeks before your average last frost date";
        let result = extract_weeks_pattern(text).unwrap();
        assert_eq!(result.weeks_min, 2);
        assert_eq!(result.weeks_max, 4);
        assert!(matches!(result.relative_timing, RelativeTiming::Before));
        assert!(matches!(result.timing_type, TimingType::LastFrost));

        // Test after last frost
        let text = "1 to 2 weeks after your average last frost date";
        let result = extract_weeks_pattern(text).unwrap();
        assert_eq!(result.weeks_min, 1);
        assert_eq!(result.weeks_max, 2);
        assert!(matches!(result.relative_timing, RelativeTiming::After));
        assert!(matches!(result.timing_type, TimingType::LastFrost));

        // Test before transplanting
        let text = "6 to 8 weeks before transplanting";
        let result = extract_weeks_pattern(text).unwrap();
        assert_eq!(result.weeks_min, 6);
        assert_eq!(result.weeks_max, 8);
        assert!(matches!(result.relative_timing, RelativeTiming::Before));
        assert!(matches!(result.timing_type, TimingType::Transplant));

        // Test invalid format
        let text = "plant whenever you feel like it";
        assert!(extract_weeks_pattern(text).is_none());
    }

    #[test]
    fn test_calculate_start_date() {
        let frost_date = NaiveDate::from_ymd_opt(2025, 5, 10).unwrap();

        // Test before last frost
        let sowing_time = SowingTime {
            weeks_min: 2,
            weeks_max: 4,
            relative_timing: RelativeTiming::Before,
            timing_type: TimingType::LastFrost,
        };
        let result = calculate_start_date(&sowing_time, frost_date);
        assert_eq!(result, NaiveDate::from_ymd_opt(2025, 4, 26).unwrap()); // 2 weeks before May 10

        // Test after last frost
        let sowing_time = SowingTime {
            weeks_min: 1,
            weeks_max: 2,
            relative_timing: RelativeTiming::After,
            timing_type: TimingType::LastFrost,
        };
        let result = calculate_start_date(&sowing_time, frost_date);
        assert_eq!(result, NaiveDate::from_ymd_opt(2025, 5, 17).unwrap()); // 1 week after May 10

        // Test before transplant
        let sowing_time = SowingTime {
            weeks_min: 6,
            weeks_max: 8,
            relative_timing: RelativeTiming::Before,
            timing_type: TimingType::Transplant,
        };
        let result = calculate_start_date(&sowing_time, frost_date);
        let transplant_date = frost_date + Days::new(21); // 3 weeks after frost date
        assert_eq!(result, transplant_date - Days::new(42)); // 6 weeks before transplant

        // Test after transplant
        let sowing_time = SowingTime {
            weeks_min: 1,
            weeks_max: 2,
            relative_timing: RelativeTiming::After,
            timing_type: TimingType::Transplant,
        };
        let result = calculate_start_date(&sowing_time, frost_date);
        let transplant_date = frost_date + Days::new(21); // 3 weeks after frost date
        assert_eq!(result, transplant_date + Days::new(7)); // 1 week after transplant
    }

    #[test]
    fn test_get_when_to_seed_start() {
        let mut info = PlantInfo {
            url: "test".to_string(),
            title: None,
            description: None,
            days_to_maturity: None,
            family: None,
            plant_type: None,
            native: None,
            hardiness: None,
            exposure: None,
            plant_dimensions: None,
            variety_info: None,
            attributes: None,
            when_to_sow_outside: Some(
                "2 to 4 weeks before your average last frost date".to_string(),
            ),
            when_to_start_inside: None,
            days_to_emerge: None,
            seed_depth: None,
            seed_spacing: None,
            row_spacing: None,
            thinning: None,
            rating: None,
            votes: None,
        };

        // Test outside sowing before frost
        let result = get_when_to_seed_start(&info).unwrap();
        assert_eq!(result.weeks_min, 2);
        assert_eq!(result.weeks_max, 4);
        assert!(matches!(result.relative_timing, RelativeTiming::Before));
        assert!(matches!(result.timing_type, TimingType::LastFrost));

        // Test outside sowing after frost
        info.when_to_sow_outside =
            Some("1 to 2 weeks after your average last frost date".to_string());
        let result = get_when_to_seed_start(&info).unwrap();
        assert_eq!(result.weeks_min, 1);
        assert_eq!(result.weeks_max, 2);
        assert!(matches!(result.relative_timing, RelativeTiming::After));
        assert!(matches!(result.timing_type, TimingType::LastFrost));

        // Test inside sowing before transplant
        info.when_to_start_inside =
            Some("RECOMMENDED. 6 to 8 weeks before transplanting".to_string());
        let result = get_when_to_seed_start(&info).unwrap();
        assert_eq!(result.weeks_min, 6);
        assert_eq!(result.weeks_max, 8);
        assert!(matches!(result.relative_timing, RelativeTiming::Before));
        assert!(matches!(result.timing_type, TimingType::Transplant));

        // Test no pattern found
        info.when_to_sow_outside = Some("Direct sow in spring".to_string());
        info.when_to_start_inside = None;
        assert!(get_when_to_seed_start(&info).is_none());
    }
}
