//! Traveller details for a booking: who is flying and on which passport.
//!
//! Name and email come from the customer's T3N profile: `{{profile.*}}`
//! markers the host fills in, so they never enter this contract. The profile
//! holds nothing else Duffel needs, so the customer supplies the rest: title
//! and phone in a traveller record they save with `save-traveller`
//! (`z:<tid>:travellers`), and the passport, including date of birth and sex,
//! either in that record or sealed with the Private Doc Scanner
//! (`z:<tid>:intake`).
//!
//! Everything here is pure, so it runs under `cargo test`. Errors name the
//! field, never the value.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::countries::ALPHA3_TO_ALPHA2;

pub const TRAVELLERS_MAP_TAIL: &str = "travellers";
/// The Private Doc Scanner's vault map, in the same tenant.
pub const SCANNER_INTAKE_MAP_TAIL: &str = "intake";

const TITLES: [&str; 5] = ["mr", "ms", "mrs", "miss", "dr"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Traveller {
    pub title: String,
    pub phone_number: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passport: Option<Passport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Passport {
    pub number: String,
    /// ISO 3166-1 alpha-2 (`GB`) or alpha-3 (`GBR`); stored as alpha-2.
    pub issuing_country: String,
    pub expires_on: String,
    pub date_of_birth: String,
    /// `M` or `F` as printed; stored as Duffel's `m` / `f`.
    pub sex: String,
}

pub type Date = (i64, u32, u32);

/// Parse and validate a `save-traveller` input, normalising the passport's
/// country to alpha-2.
pub fn parse_traveller(bytes: &[u8]) -> Result<Traveller, String> {
    let mut t: Traveller = serde_json::from_slice(bytes).map_err(|e| {
        format!("save-traveller: not a traveller record (line {}, column {})", e.line(), e.column())
    })?;
    let bad = |field: &str, why: &str| Err(format!("save-traveller: {field}: {why}"));
    if !TITLES.contains(&t.title.as_str()) {
        return bad("title", "must be one of mr, ms, mrs, miss, dr");
    }
    if !is_e164(&t.phone_number) {
        return bad("phone_number", "must be in international format, e.g. +442071234567");
    }
    if let Some(p) = &mut t.passport {
        if p.number.trim().is_empty() {
            return bad("passport.number", "must not be empty");
        }
        if parse_date(&p.expires_on).is_none() {
            return bad("passport.expires_on", "must be a date in YYYY-MM-DD form");
        }
        if parse_date(&p.date_of_birth).is_none() {
            return bad("passport.date_of_birth", "must be a date in YYYY-MM-DD form");
        }
        p.sex = duffel_gender(&p.sex)
            .ok_or_else(|| "save-traveller: passport.sex: must be M or F; Duffel can't book a passport with unspecified sex".to_string())?
            .to_string();
        p.issuing_country = alpha2(&p.issuing_country)
            .ok_or_else(|| "save-traveller: passport.issuing_country: must be an ISO country code, e.g. GB or GBR".to_string())?
            .to_string();
    }
    Ok(t)
}

/// The passport from a Private Doc Scanner record (`z:<tid>:intake`).
/// The record must belong to `caller_hex`, and its passport number, expiry
/// and issuing country must have passed the scanner's MRZ checks unedited.
pub fn passport_from_scanner(record: &[u8], caller_hex: &str) -> Result<Passport, String> {
    #[derive(Deserialize)]
    struct Record {
        owner: String,
        submission: Submission,
    }
    #[derive(Deserialize)]
    struct Submission {
        documents: Vec<Document>,
    }
    #[derive(Deserialize)]
    struct Document {
        doc_type: String,
        fields: alloc::collections::BTreeMap<String, Field>,
    }
    #[derive(Deserialize)]
    struct Field {
        value: String,
        validated: bool,
        #[serde(default)]
        user_edited: bool,
    }

    let not_found = || "book-offer: no scanned passport found for this reference".to_string();
    let record: Record = serde_json::from_slice(record).map_err(|_| "book-offer: scanned record could not be read".to_string())?;
    if !record.owner.eq_ignore_ascii_case(caller_hex) {
        return Err(not_found());
    }
    let doc = record.submission.documents.iter().find(|d| d.doc_type == "passport").ok_or_else(not_found)?;
    let field = |key: &str| -> Result<&str, String> {
        let f = doc.fields.get(key).ok_or_else(|| format!("book-offer: scanned passport has no {key}"))?;
        if !f.validated || f.user_edited {
            return Err(format!(
                "book-offer: scanned passport {key} was not verified by the scanner; rescan it or save the passport with save-traveller"
            ));
        }
        Ok(f.value.as_str())
    };
    let number = field("document_number")?.to_string();
    let expires_on = field("date_of_expiry")?.to_string();
    if parse_date(&expires_on).is_none() {
        return Err("book-offer: scanned passport date_of_expiry is not a YYYY-MM-DD date".to_string());
    }
    let issuing_country = alpha2(field("issuing_country")?)
        .ok_or_else(|| "book-offer: scanned passport issuing_country is not a supported country code".to_string())?
        .to_string();
    let date_of_birth = field("date_of_birth")?.to_string();
    if parse_date(&date_of_birth).is_none() {
        return Err("book-offer: scanned passport date_of_birth is not a YYYY-MM-DD date".to_string());
    }
    let sex = duffel_gender(field("sex")?)
        .ok_or_else(|| "book-offer: scanned passport sex is not M or F (male or female), which Duffel requires; a passport with unspecified sex can't be booked through Duffel".to_string())?
        .to_string();
    Ok(Passport { number, issuing_country, expires_on, date_of_birth, sex })
}

/// The Duffel passenger object: name and email from the customer's T3N
/// profile (`{{profile.*}}` markers the host resolves), the rest from the
/// traveller record and passport.
pub fn passenger(passenger_id: &str, t: &Traveller, passport: &Passport, today: Date) -> Result<Value, String> {
    let expiry = parse_date(&passport.expires_on).ok_or_else(|| "book-offer: passport expires_on is not a date".to_string())?;
    if expiry < today {
        return Err("book-offer: the passport has expired".to_string());
    }
    Ok(json!({
        "id": passenger_id,
        "title": t.title,
        "phone_number": t.phone_number,
        "given_name": "{{profile.first_name}}",
        "family_name": "{{profile.last_name}}",
        "born_on": passport.date_of_birth,
        "gender": passport.sex,
        "email": "{{profile.verified_contacts.email.value}}",
        "identity_documents": [{
            "type": "passport",
            "unique_identifier": passport.number,
            "issuing_country_code": passport.issuing_country,
            "expires_on": passport.expires_on,
        }],
    }))
}

/// Duffel's `gender`, which accepts only "m" or "f". Takes the passport's
/// letter (M/F, as typed in a traveller record) or the word the Private Doc
/// Scanner stores ("male"/"female", as its MRZ parser reports it). A passport
/// with unspecified sex ("<" in the MRZ, "nonspecified" from the scanner)
/// can't be booked through Duffel, so it maps to None and is refused before
/// any request is sent.
fn duffel_gender(sex: &str) -> Option<&'static str> {
    match sex.trim().to_ascii_uppercase().as_str() {
        "M" | "MALE" => Some("m"),
        "F" | "FEMALE" => Some("f"),
        _ => None,
    }
}

pub fn alpha2(code: &str) -> Option<&'static str> {
    let code = code.trim().to_ascii_uppercase();
    if code.len() == 2 {
        return ALPHA3_TO_ALPHA2.iter().find(|(_, a2)| *a2 == code).map(|(_, a2)| *a2);
    }
    ALPHA3_TO_ALPHA2
        .binary_search_by(|(a3, _)| (*a3).cmp(code.as_str()))
        .ok()
        .map(|i| ALPHA3_TO_ALPHA2[i].1)
}

fn is_e164(s: &str) -> bool {
    let digits = s.strip_prefix('+').unwrap_or("");
    (7..=15).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
}

pub fn parse_date(s: &str) -> Option<Date> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: u32 = s[5..7].parse().ok()?;
    let d: u32 = s[8..10].parse().ok()?;
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _ => return None,
    };
    (1..=days).contains(&d).then_some((y, m, d))
}

/// Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
pub fn civil_from_days(z: i64) -> Date {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn today(cluster_secs: u64) -> Date {
    civil_from_days((cluster_secs / 86_400) as i64)
}
