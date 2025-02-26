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
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Process a CSV file containing plant names and URLs
    Batch {
        #[arg(short, long)]
        file: String,
        #[arg(short, long)]
        json_dir: String,
    },
    /// Export data from JSON files to CSV, using input CSV for additional columns
    Export {
        #[arg(short, long)]
        input_file: String,
        #[arg(short, long)]
        output_file: String,
        #[arg(short, long)]
        json_dir: String,
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

// Create a reusable HTTP client with standard headers
fn create_http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8".parse().unwrap(),
            );
            headers.insert(
                reqwest::header::ACCEPT_LANGUAGE,
                "en-US,en;q=0.5".parse().unwrap(),
            );
            headers.insert(
                reqwest::header::CONNECTION,
                "keep-alive".parse().unwrap(),
            );
            headers
        })
        .build()
        .expect("Failed to create HTTP client")
}

#[derive(Debug, Clone, Copy)]
enum TimingType {
    LastFrost,
    Transplant,
}

#[derive(Debug, Clone, Copy)]
struct SowingTime {
    weeks_min: i64,
    weeks_max: i64,
    relative_timing: RelativeTiming,
    timing_type: TimingType,
}

#[derive(Debug, Clone, Copy)]
enum RelativeTiming {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SowingStrategy {
    Inside,
    Outside,
}

// Add a method to convert SowingStrategy to string for display
impl SowingStrategy {
    fn to_string(&self) -> String {
        match self {
            SowingStrategy::Inside => "Inside".to_string(),
            SowingStrategy::Outside => "Outside".to_string(),
        }
    }
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

fn determine_sowing_strategy(
    info: &PlantInfo,
    user_strategy: Option<SowingStrategy>,
) -> Option<SowingStrategy> {
    // If user provided a strategy, use it
    if let Some(strategy) = user_strategy {
        return Some(strategy);
    }

    // Otherwise use the derived strategy
    match (
        info.when_to_sow_outside.as_deref(),
        info.when_to_start_inside.as_deref(),
    ) {
        (Some(out), _) if out.contains("RECOMMENDED") => Some(SowingStrategy::Outside),
        (_, Some(ins)) if ins.contains("RECOMMENDED") => Some(SowingStrategy::Inside),
        (Some(_), None) => Some(SowingStrategy::Outside), // Default to outside if no inside instructions
        (None, Some(_)) => Some(SowingStrategy::Inside), // Default to inside if no outside instructions
        (Some(_), Some(_)) => Some(SowingStrategy::Outside), // Default to outside if both are available but no recommendation
        _ => None,
    }
}

fn get_when_to_seed_start(
    info: &PlantInfo,
    user_strategy: Option<SowingStrategy>,
) -> Option<SowingTime> {
    // Use the default logic based on recommended strategy
    let strategy = determine_sowing_strategy(info, user_strategy);
    let text = match strategy {
        Some(SowingStrategy::Inside) => info.when_to_start_inside.as_deref(),
        Some(SowingStrategy::Outside) => info.when_to_sow_outside.as_deref(),
        None => None,
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

// Helper function to get field with NULL fallback
fn get_field<T: AsRef<str>>(option: &Option<T>) -> &str {
    option.as_ref().map(|s| s.as_ref()).unwrap_or("NULL")
}

// Helper function to create error records for plants with missing JSON
fn create_error_record<'a>(input: &'a InputRecord) -> Vec<&'a str> {
    let mut row = vec![
        input.plant_name,
        input.url,
        input.brand,
        input.purchase_year,
        input.notes,
        input.user_strategy_str,
    ];
    row.extend(vec!["ERR"; 24]); // 24 columns of scraped data
    row
}

// Struct to represent an input CSV record
struct InputRecord<'a> {
    plant_name: &'a str,
    url: &'a str,
    brand: &'a str,
    purchase_year: &'a str,
    notes: &'a str,
    user_strategy_str: &'a str,
    user_strategy: Option<SowingStrategy>,
}

impl<'a> InputRecord<'a> {
    // Create a new InputRecord from a CSV record
    fn from_csv_record(record: &'a csv::StringRecord) -> Self {
        let plant_name = record.get(0).unwrap_or("unknown");
        let url = record.get(1).unwrap_or("");
        let brand = record.get(2).unwrap_or("");
        let purchase_year = record.get(3).unwrap_or("");
        let notes = record.get(4).unwrap_or("");
        let user_strategy_str = record.get(5).unwrap_or("");

        // Parse the user strategy string
        let user_strategy = match user_strategy_str {
            "Inside" => Some(SowingStrategy::Inside),
            "Outside" => Some(SowingStrategy::Outside),
            _ if user_strategy_str.trim().is_empty() => None,
            _ => None,
        };

        InputRecord {
            plant_name,
            url,
            brand,
            purchase_year,
            notes,
            user_strategy_str,
            user_strategy,
        }
    }

    // Check if this plant has JSON data
    fn has_json_data(&self, json_dir: &str) -> bool {
        let json_path = format!("{}/{}.json", json_dir, self.plant_name.replace("/", "_"));
        Path::new(&json_path).exists()
    }

    // Get the path to the JSON file for this plant
    fn json_path(&self, json_dir: &str) -> String {
        format!("{}/{}.json", json_dir, self.plant_name.replace("/", "_"))
    }

    // Validate URL is not empty for scraping
    fn has_valid_url(&self) -> bool {
        !self.url.trim().is_empty()
    }
}

// Struct to represent a complete output CSV record
struct OutputRecord<'a> {
    // Input CSV fields
    plant_name: &'a str,
    url: &'a str,
    brand: &'a str,
    purchase_year: &'a str,
    notes: &'a str,
    user_strategy: &'a str,

    // Plant info fields
    title: &'a str,
    description: &'a str,
    days_to_maturity: &'a str,
    family: &'a str,
    plant_type: &'a str,
    native: &'a str,
    hardiness: &'a str,
    exposure: &'a str,
    plant_dimensions: &'a str,
    variety_info: &'a str,
    attributes: &'a str,
    when_to_sow_outside: &'a str,
    when_to_start_inside: &'a str,
    days_to_emerge: &'a str,
    seed_depth: &'a str,
    seed_spacing: &'a str,
    row_spacing: &'a str,
    thinning: &'a str,

    // Owned fields that need to be String
    rating: String,
    votes: String,
    sowing_strategy: String,
    when_to_seed_start: String,
    calculated_start_date: String,
}

impl<'a> OutputRecord<'a> {
    // Create a new OutputRecord with all fields
    fn new(
        input: &'a InputRecord<'a>,
        info: &'a PlantInfo,
        sowing_strategy: Option<SowingStrategy>,
        when_to_start_str: String,
        start_date: String,
    ) -> Self {
        OutputRecord {
            // Input CSV fields
            plant_name: input.plant_name,
            url: input.url,
            brand: input.brand,
            purchase_year: input.purchase_year,
            notes: input.notes,
            user_strategy: input.user_strategy_str,

            // Plant info fields
            title: get_field(&info.title),
            description: get_field(&info.description),
            days_to_maturity: get_field(&info.days_to_maturity),
            family: get_field(&info.family),
            plant_type: get_field(&info.plant_type),
            native: get_field(&info.native),
            hardiness: get_field(&info.hardiness),
            exposure: get_field(&info.exposure),
            plant_dimensions: get_field(&info.plant_dimensions),
            variety_info: get_field(&info.variety_info),
            attributes: get_field(&info.attributes),
            when_to_sow_outside: get_field(&info.when_to_sow_outside),
            when_to_start_inside: get_field(&info.when_to_start_inside),
            days_to_emerge: get_field(&info.days_to_emerge),
            seed_depth: get_field(&info.seed_depth),
            seed_spacing: get_field(&info.seed_spacing),
            row_spacing: get_field(&info.row_spacing),
            thinning: get_field(&info.thinning),

            // Owned fields
            rating: info
                .rating
                .map_or_else(|| "NULL".to_string(), |r| r.to_string()),
            votes: info
                .votes
                .map_or_else(|| "NULL".to_string(), |v| v.to_string()),
            sowing_strategy: sowing_strategy
                .as_ref()
                .map_or_else(|| "NULL".to_string(), |s| s.to_string()),
            when_to_seed_start: when_to_start_str,
            calculated_start_date: start_date,
        }
    }

    // Convert to a CSV record
    fn to_record(&self) -> Vec<String> {
        vec![
            self.plant_name.to_string(),
            self.url.to_string(),
            self.brand.to_string(),
            self.purchase_year.to_string(),
            self.notes.to_string(),
            self.user_strategy.to_string(),
            self.title.to_string(),
            self.description.to_string(),
            self.days_to_maturity.to_string(),
            self.family.to_string(),
            self.plant_type.to_string(),
            self.native.to_string(),
            self.hardiness.to_string(),
            self.exposure.to_string(),
            self.plant_dimensions.to_string(),
            self.variety_info.to_string(),
            self.attributes.to_string(),
            self.when_to_sow_outside.to_string(),
            self.when_to_start_inside.to_string(),
            self.days_to_emerge.to_string(),
            self.seed_depth.to_string(),
            self.seed_spacing.to_string(),
            self.row_spacing.to_string(),
            self.thinning.to_string(),
            self.rating.clone(),
            self.votes.clone(),
            self.sowing_strategy.clone(),
            self.when_to_seed_start.clone(),
            self.calculated_start_date.clone(),
        ]
    }
}

fn process_csv(file_path: &str, json_dir: &str) -> Result<()> {
    let results_dir = Path::new(json_dir);
    if !results_dir.exists() {
        fs::create_dir(results_dir).context(format!("Failed to create directory: {}", json_dir))?;
    }

    let mut failed_plants = Vec::new();
    let mut rdr = csv::Reader::from_path(file_path)
        .context(format!("Failed to read CSV file: {}", file_path))?;

    for result in rdr.records() {
        let record = match result {
            Ok(record) => record,
            Err(e) => {
                eprintln!("Error reading CSV record: {}", e);
                continue;
            }
        };

        // Parse the input record
        let input = InputRecord::from_csv_record(&record);

        // Validate URL for scraping
        if !input.has_valid_url() {
            eprintln!("Empty URL for plant: {}", input.plant_name);
            failed_plants.push(input.plant_name.to_string());
            continue;
        }

        // Skip if file already exists
        if input.has_json_data(json_dir) {
            println!("Skipping {} - result file already exists", input.plant_name);
            continue;
        }

        println!("Processing {} from {}", input.plant_name, input.url);

        // Sleep between requests
        thread::sleep(StdDuration::from_secs(2));

        let client = create_http_client();
        let response = match client.get(input.url).send().and_then(|r| r.text()) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("Failed to fetch {}: {}", input.plant_name, e);
                failed_plants.push(input.plant_name.to_string());
                continue;
            }
        };

        match PlantInfo::from_html(&response, input.url.to_string()) {
            Ok(info) => {
                let json = match serde_json::to_string_pretty(&info) {
                    Ok(j) => j,
                    Err(e) => {
                        eprintln!("Failed to serialize JSON for {}: {}", input.plant_name, e);
                        failed_plants.push(input.plant_name.to_string());
                        continue;
                    }
                };

                if let Err(e) = fs::write(&input.json_path(json_dir), json) {
                    eprintln!("Failed to write file for {}: {}", input.plant_name, e);
                    failed_plants.push(input.plant_name.to_string());
                }
            }
            Err(e) => {
                eprintln!("Failed to parse HTML for {}: {}", input.plant_name, e);
                failed_plants.push(input.plant_name.to_string());
            }
        }
    }

    if !failed_plants.is_empty() {
        eprintln!("\nFailed to process the following plants:");
        for plant in &failed_plants {
            eprintln!("- {}", plant);
        }
    } else {
        println!("All plants processed successfully.");
    }

    println!("JSON results saved to directory: {}", json_dir);
    Ok(())
}

fn export_to_csv(input_file: &str, output_file: &str, json_dir: &str) -> Result<()> {
    let results_dir = Path::new(json_dir);
    if !results_dir.exists() {
        return Err(anyhow::anyhow!("Directory {} does not exist", json_dir));
    }

    // Read the input CSV file
    let mut input_rdr = csv::Reader::from_path(input_file)
        .context(format!("Failed to read input CSV file: {}", input_file))?;

    let mut writer = csv::Writer::from_path(output_file)?;

    // Write headers - include the original columns plus the scraped data
    writer.write_record(&[
        "Plant Name",
        "URL",
        "Brand",                 // New column
        "Purchase Year",         // New column
        "Notes",                 // New column
        "Users Sowing Strategy", // New column to be preserved
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
    let mut processed_count = 0;
    let mut missing_json_count = 0;

    // Process each row in the input CSV
    for result in input_rdr.records() {
        let record = match result {
            Ok(record) => record,
            Err(e) => {
                eprintln!("Error reading CSV record: {}", e);
                continue;
            }
        };

        // Parse the input record
        let input = InputRecord::from_csv_record(&record);

        // Check if JSON data exists for this plant
        if !input.has_json_data(json_dir) {
            eprintln!(
                "Warning: No JSON data found for plant: {}",
                input.plant_name
            );
            // Use the helper function to create the error record
            let row = create_error_record(&input);
            writer.write_record(&row)?;
            missing_json_count += 1;
            continue;
        }

        // Read and parse the JSON file
        let content = match fs::read_to_string(&input.json_path(json_dir)) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Failed to read JSON file for {}: {}", input.plant_name, e);
                continue;
            }
        };

        let info: PlantInfo = match serde_json::from_str(&content) {
            Ok(info) => info,
            Err(e) => {
                eprintln!("Failed to parse JSON for {}: {}", input.plant_name, e);
                continue;
            }
        };

        // Get the sowing strategy as an enum
        let sowing_strategy = determine_sowing_strategy(&info, input.user_strategy);

        // Get the sowing time based on the strategy enum
        let when_to_start = get_when_to_seed_start(&info, input.user_strategy);

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

        // Create an OutputRecord and write it to the CSV
        let record = OutputRecord::new(
            &input,
            &info,
            sowing_strategy,
            when_to_start_str,
            start_date,
        );

        // Convert the record to strings and write them to the CSV
        let string_record = record.to_record();
        let str_refs: Vec<&str> = string_record.iter().map(|s| s.as_str()).collect();
        writer.write_record(&str_refs)?;
        processed_count += 1;
    }

    writer.flush()?;
    println!("Exported data to {}", output_file);
    println!("Used JSON data from directory: {}", json_dir);
    println!("Used input CSV file: {}", input_file);
    println!(
        "Processed {} plants ({} with missing JSON data marked as ERR)",
        processed_count + missing_json_count,
        missing_json_count
    );
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Single { url, output } => {
            let client = create_http_client();
            let response = client
                .get(&url)
                .send()
                .context("Failed to send request")?
                .text()
                .context("Failed to get response text")?;

            match PlantInfo::from_html(&response, url) {
                Ok(info) => {
                    let json = serde_json::to_string_pretty(&info)?;
                    println!("{}", json);

                    if let Some(output_path) = output {
                        fs::write(&output_path, &json)
                            .context(format!("Failed to write output to {}", output_path))?;
                        println!("Results saved to: {}", output_path);
                    }
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
        Commands::Batch { file, json_dir } => {
            process_csv(&file, &json_dir)?;
        }
        Commands::Export {
            input_file,
            output_file,
            json_dir,
        } => {
            export_to_csv(&input_file, &output_file, &json_dir)?;
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
        let info = PlantInfo {
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

        // Test with no user strategy
        let result = get_when_to_seed_start(&info, None).unwrap();
        assert_eq!(result.weeks_min, 2);
        assert_eq!(result.weeks_max, 4);
        assert!(matches!(result.relative_timing, RelativeTiming::Before));
        assert!(matches!(result.timing_type, TimingType::LastFrost));

        // Add new test cases for user strategy
        let result = get_when_to_seed_start(&info, Some(SowingStrategy::Inside)).unwrap();
        assert_eq!(result.weeks_min, 6);
        assert_eq!(result.weeks_max, 8);
        assert!(matches!(result.relative_timing, RelativeTiming::Before));
        assert!(matches!(result.timing_type, TimingType::Transplant));
    }

    #[test]
    fn test_determine_sowing_strategy() {
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
                "RECOMMENDED. 2 to 4 weeks before your average last frost date".to_string(),
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

        // Test with outside recommended
        let result = determine_sowing_strategy(&info, None);
        assert_eq!(result, Some(SowingStrategy::Outside));

        // Test with inside recommended
        info.when_to_sow_outside = None;
        info.when_to_start_inside =
            Some("RECOMMENDED. 6 to 8 weeks before transplanting".to_string());
        let result = determine_sowing_strategy(&info, None);
        assert_eq!(result, Some(SowingStrategy::Inside));

        // Test with user strategy overriding
        let result = determine_sowing_strategy(&info, Some(SowingStrategy::Outside));
        assert_eq!(result, Some(SowingStrategy::Outside));
    }

    #[test]
    fn test_input_record_from_csv() {
        // Create a mock CSV record
        let record = csv::StringRecord::from(vec![
            "Carrot",
            "http://example.com",
            "Brand X",
            "2023",
            "Test notes",
            "Inside",
        ]);

        let input = InputRecord::from_csv_record(&record);

        assert_eq!(input.plant_name, "Carrot");
        assert_eq!(input.url, "http://example.com");
        assert_eq!(input.brand, "Brand X");
        assert_eq!(input.purchase_year, "2023");
        assert_eq!(input.notes, "Test notes");
        assert_eq!(input.user_strategy_str, "Inside");
        assert_eq!(input.user_strategy, Some(SowingStrategy::Inside));
    }

    #[test]
    fn test_input_record_with_empty_strategy() {
        // Create a mock CSV record with empty strategy
        let record = csv::StringRecord::from(vec![
            "Carrot",
            "http://example.com",
            "Brand X",
            "2023",
            "Test notes",
            "",
        ]);

        let input = InputRecord::from_csv_record(&record);

        assert_eq!(input.user_strategy_str, "");
        assert_eq!(input.user_strategy, None);
    }

    #[test]
    fn test_create_error_record() {
        // Create a mock input record
        let record = csv::StringRecord::from(vec![
            "Carrot",
            "http://example.com",
            "Brand X",
            "2023",
            "Test notes",
            "Inside",
        ]);

        let input = InputRecord::from_csv_record(&record);

        // Create error record
        let error_record = create_error_record(&input);

        // Check the first 6 fields come from input
        assert_eq!(error_record[0], "Carrot");
        assert_eq!(error_record[1], "http://example.com");
        assert_eq!(error_record[2], "Brand X");
        assert_eq!(error_record[3], "2023");
        assert_eq!(error_record[4], "Test notes");
        assert_eq!(error_record[5], "Inside");

        // Check that we have 24 ERR fields
        assert_eq!(error_record.len(), 30); // 6 input fields + 24 ERR fields
        assert_eq!(error_record[6], "ERR");
        assert_eq!(error_record[29], "ERR");
    }

    #[test]
    fn test_output_record_creation() {
        // Create a mock input record
        let record = csv::StringRecord::from(vec![
            "Carrot",
            "http://example.com",
            "Brand X",
            "2023",
            "Test notes",
            "Inside",
        ]);

        let input = InputRecord::from_csv_record(&record);

        // Create a mock PlantInfo
        let info = PlantInfo {
            url: "http://example.com".to_string(),
            title: Some("Test Carrot".to_string()),
            description: Some("A test carrot description".to_string()),
            days_to_maturity: Some("70 days".to_string()),
            family: Some("Apiaceae".to_string()),
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
            rating: Some(4.5),
            votes: Some(10),
        };

        // Create OutputRecord
        let output = OutputRecord::new(
            &input,
            &info,
            Some(SowingStrategy::Inside),
            "6-8 before TRANSPLANT".to_string(),
            "2025-03-15".to_string(),
        );

        // Verify input fields are copied correctly
        assert_eq!(output.plant_name, "Carrot");
        assert_eq!(output.url, "http://example.com");
        assert_eq!(output.brand, "Brand X");
        assert_eq!(output.purchase_year, "2023");
        assert_eq!(output.notes, "Test notes");
        assert_eq!(output.user_strategy, "Inside");

        // Verify plant info fields
        assert_eq!(output.title, "Test Carrot");
        assert_eq!(output.description, "A test carrot description");
        assert_eq!(output.days_to_maturity, "70 days");
        assert_eq!(output.family, "Apiaceae");

        // Verify calculated fields
        assert_eq!(output.rating, "4.5");
        assert_eq!(output.votes, "10");
        assert_eq!(output.sowing_strategy, "Inside");
        assert_eq!(output.when_to_seed_start, "6-8 before TRANSPLANT");
        assert_eq!(output.calculated_start_date, "2025-03-15");

        // Verify converted to record
        let record_vec = output.to_record();
        assert_eq!(record_vec[0], "Carrot");
        assert_eq!(record_vec[6], "Test Carrot");
        assert_eq!(record_vec[26], "Inside");
    }
}
