//! Fee receipt PDFs, rendered with Typst from `templates/receipt.typ`.

use acadex_db::fees::ReceiptDocument;
use chrono_tz::Asia::Kolkata;
use serde_json::json;
use typst::foundations::{Dict, IntoValue};
use typst_as_lib::{TypstEngine, TypstTemplateMainFile};
use typst_layout::PagedDocument;

const TEMPLATE: &str = include_str!("../templates/receipt.typ");

/// Holds the parsed template and loaded fonts, so each receipt only pays for
/// layout. Build once and share.
pub struct Renderer {
    engine: TypstEngine<TypstTemplateMainFile>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            engine: TypstEngine::builder()
                .main_file(TEMPLATE)
                .fonts(typst_assets::fonts())
                .build(),
        }
    }
}

impl Renderer {
    /// CPU-bound; call from a blocking thread.
    pub fn render(&self, receipt: &ReceiptDocument) -> anyhow::Result<Vec<u8>> {
        let mut inputs = Dict::new();
        inputs.insert(
            "receipt".into(),
            template_data(receipt).to_string().into_value(),
        );
        let document: PagedDocument = self
            .engine
            .compile_with_input(inputs)
            .output
            .map_err(|err| anyhow::anyhow!("receipt template: {err:?}"))?;
        typst_pdf::pdf(&document, &Default::default())
            .map_err(|err| anyhow::anyhow!("receipt PDF export: {err:?}"))
    }
}

/// What the template prints, already formatted.
fn template_data(receipt: &ReceiptDocument) -> serde_json::Value {
    let fy = receipt.financial_year;
    json!({
        "institution": receipt.institution,
        "receipt_no": receipt.receipt_no,
        "issued_at": receipt.issued_at.with_timezone(&Kolkata).format("%-d %b %Y, %-I:%M %p").to_string(),
        "student_name": receipt.student_name,
        "roll_no": receipt.roll_no,
        "program": receipt.program,
        "items": receipt.items.iter().map(|item| json!({
            "particulars": item.particulars,
            "amount": format_inr(item.amount_paise),
        })).collect::<Vec<_>>(),
        "total": format_inr(receipt.amount_paise),
        "total_words": amount_in_words(receipt.amount_paise),
        "method": match receipt.method.as_deref() {
            Some("upi") => "UPI",
            Some("card") => "card",
            Some("netbanking") => "net banking",
            _ => "the payment gateway",
        },
        "reference": receipt.gateway_payment_id,
        "financial_year": format!("FY {fy}-{:02}", (fy + 1) % 100),
    })
}

/// Indian digit grouping: 6490000 paise → "₹64,900.00"; 1 crore → "₹1,00,00,000.00".
pub fn format_inr(paise: i64) -> String {
    let rupees = (paise / 100).unsigned_abs().to_string();
    let (rest, last3) = rupees.split_at(rupees.len().saturating_sub(3));
    let mut groups: Vec<&str> = Vec::new();
    let mut rest = rest;
    while rest.len() > 2 {
        let (head, tail) = rest.split_at(rest.len() - 2);
        groups.push(tail);
        rest = head;
    }
    if !rest.is_empty() {
        groups.push(rest);
    }
    groups.reverse();
    groups.push(last3);
    let sign = if paise < 0 { "-" } else { "" };
    format!("{sign}₹{}.{:02}", groups.join(","), (paise % 100).abs())
}

/// "Rupees Sixty-Four Thousand Nine Hundred Only", in the Indian system
/// (thousand, lakh, crore), as printed on receipts and cheques.
pub fn amount_in_words(paise: i64) -> String {
    let paise = paise.unsigned_abs();
    let (rupees, paise) = (paise / 100, paise % 100);
    let mut words = format!("Rupees {}", number_in_words(rupees));
    if paise > 0 {
        words.push_str(&format!(" and {} Paise", number_in_words(paise)));
    }
    words + " Only"
}

fn number_in_words(n: u64) -> String {
    const ONES: [&str; 20] = [
        "Zero",
        "One",
        "Two",
        "Three",
        "Four",
        "Five",
        "Six",
        "Seven",
        "Eight",
        "Nine",
        "Ten",
        "Eleven",
        "Twelve",
        "Thirteen",
        "Fourteen",
        "Fifteen",
        "Sixteen",
        "Seventeen",
        "Eighteen",
        "Nineteen",
    ];
    const TENS: [&str; 10] = [
        "", "", "Twenty", "Thirty", "Forty", "Fifty", "Sixty", "Seventy", "Eighty", "Ninety",
    ];
    fn below_hundred(n: u64) -> String {
        match n {
            0..20 => ONES[n as usize].to_owned(),
            _ if n.is_multiple_of(10) => TENS[(n / 10) as usize].to_owned(),
            _ => format!("{}-{}", TENS[(n / 10) as usize], ONES[(n % 10) as usize]),
        }
    }
    if n == 0 {
        return ONES[0].to_owned();
    }
    let mut parts = Vec::new();
    let mut n = n;
    for (size, name) in [
        (10_000_000, "Crore"),
        (100_000, "Lakh"),
        (1_000, "Thousand"),
        (100, "Hundred"),
    ] {
        if n >= size {
            // Above a crore the count is itself a number in words.
            let count = n / size;
            let count = if count >= 100 {
                number_in_words(count)
            } else {
                below_hundred(count)
            };
            parts.push(format!("{count} {name}"));
            n %= size;
        }
    }
    if n > 0 {
        parts.push(below_hundred(n));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use acadex_db::fees::ReceiptItem;
    use chrono::TimeZone;
    use sqlx::types::Json;

    use super::*;

    #[test]
    fn indian_digit_grouping() {
        assert_eq!(format_inr(100), "₹1.00");
        assert_eq!(format_inr(240_000), "₹2,400.00");
        assert_eq!(format_inr(6_490_000), "₹64,900.00");
        assert_eq!(format_inr(12_500_050), "₹1,25,000.50");
        assert_eq!(format_inr(1_000_000_000), "₹1,00,00,000.00");
    }

    #[test]
    fn amounts_in_words() {
        assert_eq!(
            amount_in_words(6_490_000),
            "Rupees Sixty-Four Thousand Nine Hundred Only"
        );
        assert_eq!(
            amount_in_words(12_500_050),
            "Rupees One Lakh Twenty-Five Thousand and Fifty Paise Only"
        );
        assert_eq!(amount_in_words(1_000_000_000), "Rupees One Crore Only");
        assert_eq!(
            amount_in_words(240_000),
            "Rupees Two Thousand Four Hundred Only"
        );
    }

    #[test]
    fn renders_a_pdf_with_the_receipt_text() {
        let receipt = ReceiptDocument {
            receipt_no: "RCPT/2026-27/000042".to_owned(),
            financial_year: 2026,
            issued_at: chrono::Utc.with_ymd_and_hms(2026, 10, 3, 6, 30, 0).unwrap(),
            amount_paise: 6_490_000,
            pdf_object: None,
            institution: "Acadex Institute of Technology".to_owned(),
            // Markup characters stay literal text.
            student_name: "Atharva *Sharma* #1".to_owned(),
            roll_no: "ECE24001".to_owned(),
            program: "B.Tech ECE".to_owned(),
            method: Some("upi".to_owned()),
            gateway_payment_id: "pay_abc123".to_owned(),
            items: Json(vec![
                ReceiptItem {
                    particulars: "Tuition fee — Semester 5".to_owned(),
                    amount_paise: 6_250_000,
                },
                ReceiptItem {
                    particulars: "Examination fee — Semester 5".to_owned(),
                    amount_paise: 240_000,
                },
            ]),
        };
        let pdf = Renderer::default().render(&receipt).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.len() > 1000);
    }
}
