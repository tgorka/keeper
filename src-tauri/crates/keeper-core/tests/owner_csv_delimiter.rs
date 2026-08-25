//! The owner's real attachment, through the shipped detector.
//!
//! Not a fixture resembling their file — the first three records of
//! `10-notes/attachments/username-password-recovery-code.csv` off neuradrive,
//! byte for byte, including the space after the first `Username;`.
use keeper_core::notes::csv;

const OWNER: &str = "Username; Identifier;One-time password;Recovery code;First name;Last name;Department;Location\nbooker12;9012;12se74;rb9012;Rachel;Booker;Sales;Manchester\ngrey07;2070;04ap67;lg2070;Laura;Grey;Depot;London\n";

#[test]
fn the_owners_semicolon_export_reads_as_eight_columns() {
    let delimiter = csv::detect_delimiter(OWNER);
    assert_eq!(delimiter, b';', "the file the owner showed on screen");

    let rows = csv::table_rows(OWNER, delimiter);
    assert_eq!(rows.len(), 3);
    // Eight columns, which is what the owner saw collapsed into one.
    assert_eq!(rows[0].len(), 8);
    assert_eq!(rows[0][0], "Username");
    // The space after `Username;` belongs to the next field as written; keeper
    // does not trim it, because a trim is an edit to somebody's data.
    assert_eq!(rows[0][1], " Identifier");
    assert_eq!(rows[0][7], "Location");
    assert_eq!(rows[1][5], "Booker");
    assert_eq!(rows[2][7], "London");

    // Under the old behaviour every record was one field.
    assert_ne!(csv::table_rows(OWNER, b',')[0].len(), 8);
}
