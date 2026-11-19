use z_tenant_flight::traveller::*;

const MANUAL: &[u8] = include_bytes!("../examples/traveller.json");
const SCANNER_ONLY: &[u8] = include_bytes!("../examples/traveller-scanner.json");
const SCANNED: &[u8] = include_bytes!("../examples/scanner-record.json");
const OWNER: &str = "1111111111111111111111111111111111111111";
const TODAY: Date = (2026, 11, 17);

fn edit(bytes: &[u8], f: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    f(&mut v);
    serde_json::to_vec(&v).unwrap()
}

#[test]
fn name_and_email_come_from_the_profile() {
    let t = parse_traveller(MANUAL).unwrap();
    let p = passenger("pas_1", &t, t.passport.as_ref().unwrap(), TODAY).unwrap();
    assert_eq!(p["given_name"], "{{profile.first_name}}");
    assert_eq!(p["family_name"], "{{profile.last_name}}");
    assert_eq!(p["email"], "{{profile.verified_contacts.email.value}}");
    assert_eq!(p["title"], "ms");
    assert_eq!(p["phone_number"], "+442071234567");
    assert_eq!(p["id"], "pas_1");
    assert_eq!(p["identity_documents"][0]["type"], "passport");
    assert_eq!(p["identity_documents"][0]["issuing_country_code"], "GB");
    assert_eq!(p["born_on"], "1990-04-12", "date of birth comes from the passport");
    assert_eq!(p["gender"], "f", "sex comes from the passport, in Duffel's form");
    assert!(!p.to_string().contains("profile.date_of_birth") && !p.to_string().contains("profile.gender"));
}

#[test]
fn identity_fields_cannot_be_overridden_in_the_record() {
    for field in ["given_name", "family_name", "born_on", "gender", "email"] {
        assert!(parse_traveller(&edit(MANUAL, |v| v[field] = "x".into())).is_err(), "{field} must be refused");
    }
}

#[test]
fn scanner_traveller_has_no_passport() {
    let t = parse_traveller(SCANNER_ONLY).unwrap();
    assert!(t.passport.is_none());
    let pp = passport_from_scanner(SCANNED, OWNER).unwrap();
    let p = passenger("pas_1", &t, &pp, TODAY).unwrap();
    assert_eq!(p["identity_documents"][0]["unique_identifier"], "000000000");
    assert_eq!(p["born_on"], "1990-04-12");
    assert_eq!(p["gender"], "f");
}

#[test]
fn alpha3_country_is_normalised() {
    let t = parse_traveller(&edit(MANUAL, |v| v["passport"]["issuing_country"] = "nld".into())).unwrap();
    assert_eq!(t.passport.unwrap().issuing_country, "NL");
    assert_eq!(alpha2("D"), Some("DE"));
    assert_eq!(alpha2("GBD"), Some("GB"));
    assert_eq!(alpha2("XXX"), None);
}

#[test]
fn invalid_records_are_refused_without_echoing_values() {
    for (field, value) in [("title", "lord"), ("phone_number", "0207 123")] {
        let err = parse_traveller(&edit(MANUAL, |v| v[field] = value.into())).unwrap_err();
        assert!(err.contains(field), "{err}");
        assert!(!err.contains(value), "error echoed the value: {err}");
    }
    for (field, value) in [("date_of_birth", "12/04/1990"), ("sex", "X")] {
        let err = parse_traveller(&edit(MANUAL, |v| v["passport"][field] = value.into())).unwrap_err();
        assert!(err.contains(field), "{err}");
        assert!(!err.contains(value), "error echoed the value: {err}");
    }
    let err = parse_traveller(&edit(MANUAL, |v| v["passport"]["issuing_country"] = "ZZ".into())).unwrap_err();
    assert!(err.contains("issuing_country"), "{err}");
    assert!(parse_traveller(&edit(MANUAL, |v| v["passport_number"] = "1".into())).is_err(), "unknown fields are refused");
}

#[test]
fn expired_passport_is_refused() {
    let t = parse_traveller(MANUAL).unwrap();
    let pp = t.passport.clone().unwrap();
    assert!(passenger("pas_1", &t, &pp, (2031, 4, 11)).is_ok());
    assert!(passenger("pas_1", &t, &pp, (2031, 4, 12)).unwrap_err().contains("expired"));
}

#[test]
fn scanner_passport_is_used_for_its_owner() {
    let pp = passport_from_scanner(SCANNED, OWNER).unwrap();
    assert_eq!(
        pp,
        Passport {
            number: "000000000".into(),
            issuing_country: "GB".into(),
            expires_on: "2031-04-11".into(),
            date_of_birth: "1990-04-12".into(),
            sex: "f".into(),
        }
    );
}

#[test]
fn scanner_passport_of_someone_else_is_not_found() {
    let err = passport_from_scanner(SCANNED, "2222222222222222222222222222222222222222").unwrap_err();
    assert!(err.contains("no scanned passport"), "{err}");
}

fn set_field(key: &str, attr: &str, value: serde_json::Value) -> Vec<u8> {
    edit(SCANNED, |v| v["submission"]["documents"][0]["fields"][key][attr] = value)
}

#[test]
fn unverified_or_edited_scanner_fields_are_refused() {
    // Usable when the scanner verified it or the traveller corrected it;
    // refused only when it is neither.
    let guess = edit(SCANNED, |v| {
        let f = &mut v["submission"]["documents"][0]["fields"]["document_number"];
        f["validated"] = false.into();
        f["user_edited"] = false.into();
    });
    let err = passport_from_scanner(&guess, OWNER).unwrap_err();
    assert!(err.contains("document_number"), "{err}");
    assert!(err.contains("not verified by the scanner or confirmed by you"), "{err}");
    assert!(!err.contains("000000000"), "error echoed the value: {err}");

    let corrected = edit(SCANNED, |v| {
        let f = &mut v["submission"]["documents"][0]["fields"]["date_of_expiry"];
        f["validated"] = false.into();
        f["user_edited"] = true.into();
    });
    assert_eq!(passport_from_scanner(&corrected, OWNER).unwrap().expires_on, "2031-04-11");

    let verified_and_edited = set_field("sex", "user_edited", true.into());
    assert_eq!(passport_from_scanner(&verified_and_edited, OWNER).unwrap().sex, "f");
}

#[test]
fn scanned_sex_word_or_letter_maps_to_duffel() {
    for (value, want) in [("female", "f"), ("Female", "f"), ("FEMALE", "f"), ("F", "f"), (" f ", "f"), ("male", "m"), ("M", "m")] {
        let rec = set_field("sex", "value", value.into());
        assert_eq!(passport_from_scanner(&rec, OWNER).unwrap().sex, want, "sex {value:?}");
    }
    for value in ["X", "<", ""] {
        let rec = set_field("sex", "value", value.into());
        let err = passport_from_scanner(&rec, OWNER).unwrap_err();
        assert!(err.contains("not M or F"), "sex {value:?}: {err}");
    }
}

#[test]
fn save_traveller_accepts_sex_as_a_word() {
    let t = parse_traveller(&edit(MANUAL, |v| v["passport"]["sex"] = "Female".into())).unwrap();
    assert_eq!(t.passport.unwrap().sex, "f");
    let err = parse_traveller(&edit(MANUAL, |v| v["passport"]["sex"] = "X".into())).unwrap_err();
    assert!(err.contains("must be M or F (or Male / Female)"), "{err}");
}

#[test]
fn edited_scanner_passport_still_checked() {
    // An edit is accepted, but the format and expiry checks still apply.
    let bad_date = edit(SCANNED, |v| {
        let f = &mut v["submission"]["documents"][0]["fields"]["date_of_expiry"];
        f["value"] = "11/04/2031".into();
        f["user_edited"] = true.into();
    });
    assert!(passport_from_scanner(&bad_date, OWNER).unwrap_err().contains("not a YYYY-MM-DD date"));

    let expired = edit(SCANNED, |v| {
        let f = &mut v["submission"]["documents"][0]["fields"]["date_of_expiry"];
        f["value"] = "2020-01-01".into();
        f["validated"] = false.into();
        f["user_edited"] = true.into();
    });
    let pp = passport_from_scanner(&expired, OWNER).unwrap();
    let t = parse_traveller(SCANNER_ONLY).unwrap();
    assert!(passenger("pas_1", &t, &pp, TODAY).unwrap_err().contains("expired"));

    let edited_someone_elses = set_field("sex", "user_edited", true.into());
    assert!(passport_from_scanner(&edited_someone_elses, "2222222222222222222222222222222222222222")
        .unwrap_err()
        .contains("no scanned passport"));
}

#[test]
fn today_from_cluster_seconds() {
    assert_eq!(today(0), (1970, 1, 1));
    assert_eq!(today(1_794_873_600), (2026, 11, 17));
}

#[test]
fn scanned_sex_comes_in_the_scanners_words() {
    // The scanner's MRZ parser reports "male" / "female", not the passport's letter.
    let sex = |value: &str| {
        let x = edit(SCANNED, |v| v["submission"]["documents"][0]["fields"]["sex"]["value"] = value.into());
        passport_from_scanner(&x, OWNER).map(|p| p.sex)
    };
    assert_eq!(sex("female").unwrap(), "f");
    assert_eq!(sex("male").unwrap(), "m");
    assert_eq!(sex("F").unwrap(), "f");
    assert_eq!(sex("M").unwrap(), "m");
}

#[test]
fn scanned_sex_other_than_m_or_f_is_refused() {
    // Duffel accepts only "m" or "f": an unspecified sex can't be booked.
    for value in ["<", "nonspecified", "X", ""] {
        let x = edit(SCANNED, |v| v["submission"]["documents"][0]["fields"]["sex"]["value"] = value.into());
        assert!(passport_from_scanner(&x, OWNER).unwrap_err().contains("not M or F"), "{value:?} must be refused");
    }
}

#[test]
fn typed_sex_accepts_the_letter_or_the_word() {
    for (value, want) in [("F", "f"), ("female", "f"), ("M", "m"), ("Male", "m")] {
        let t = parse_traveller(&edit(MANUAL, |v| v["passport"]["sex"] = value.into())).unwrap();
        assert_eq!(t.passport.unwrap().sex, want, "{value}");
    }
    assert!(parse_traveller(&edit(MANUAL, |v| v["passport"]["sex"] = "X".into())).is_err());
}
