//! Email templates, digest aggregation, and SMTP delivery
//! (user-management.md §Email Notifications).
//!
//! - **Templates per notification type**: [`render_notification_email`]
//!   produces an HTML body and a plain-text fallback for every
//!   `NotificationType` variant (the match is exhaustive, so a new variant
//!   cannot ship without a template).
//! - **Digest mode**: [`render_digest_email`] aggregates multiple queued
//!   notifications into a single email; [`run_digest_once`] drains the
//!   durable `email_outbox` (kv_store) at each user's configured
//!   `DigestFrequency` — Immediate sends individually, Hourly/Daily
//!   aggregate and send only when the frequency window has elapsed.
//! - **Priority filter**: enforced twice — at queue time
//!   (notification_dispatcher) and re-checked at drain time against the
//!   user's *current* `min_priority`, so raising the threshold also drops
//!   already-queued low-priority noise.
//! - **Unsubscribe link per notification type**: every email links to
//!   `{base_url}/settings/notifications?unsubscribe={type}`, where the
//!   authenticated preferences page applies the per-type email opt-out
//!   (`EmailConfig::type_overrides`) that the dispatcher honors.
//! - **Locale**: rendered in the user's configured locale
//!   (`User::locale`, BCP-47 language tag; `en` default, `de` supported).
//!
//! SMTP transport is a minimal RFC 5321 client (EHLO/MAIL FROM/RCPT TO/DATA)
//! pointed at a deployment relay (`GYRE_SMTP_HOST`, default port 25). When
//! no relay is configured the drain leaves the durable outbox intact —
//! entries are held until an operator configures SMTP (queue-and-hold, the
//! same interface contract task-112 established).

use crate::notification_dispatcher::{resolve_channels, EMAIL_OUTBOX_NS};
use crate::AppState;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{DigestFrequency, NotificationPriority};
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// KV namespace holding the per-user last-digest-sent timestamp (unix secs).
pub const DIGEST_STATE_NS: &str = "email_digest_state";

/// Bound for every SMTP socket operation (connect, command, data). A hung
/// relay must not stall the digest job indefinitely.
const SMTP_TIMEOUT_SECS: u64 = 10;

// ─── Rendered email ──────────────────────────────────────────────────────────

/// A fully rendered email ready for the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedEmail {
    pub subject: String,
    pub html: String,
    pub text: String,
}

// ─── Locale catalog (§Email Notifications: "Rendered in the user's
//     configured locale") ──────────────────────────────────────────────────────

/// Supported email locales. `en` is the default for unknown tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    De,
}

/// Resolve a BCP-47 user locale tag (`User::locale`, e.g. "en-US", "de-DE")
/// to a supported email locale. Unknown tags fall back to English.
pub fn resolve_locale(tag: &str) -> Locale {
    let primary = tag
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match primary.as_str() {
        "de" => Locale::De,
        _ => Locale::En,
    }
}

/// Localized template strings. Every user-visible email string goes through
/// here — the renderer has no hardcoded prose.
struct Strings {
    /// Prefix before the notification heading in the subject line.
    subject_prefix: &'static str,
    /// "View in Gyre" button label.
    view_button: &'static str,
    /// Priority label.
    priority_label: &'static str,
    /// Timestamp label.
    time_label: &'static str,
    /// Digest subject: "{count} notifications" heading (uses digest_heading).
    digest_heading: &'static str,
    /// Digest intro line before the item list.
    digest_intro: &'static str,
    /// Per-type unsubscribe link text.
    unsubscribe: &'static str,
    /// Footer note under the unsubscribe link.
    unsubscribe_note: &'static str,
    /// Section headings per priority band.
    priority_urgent: &'static str,
    priority_high: &'static str,
    priority_medium: &'static str,
    priority_low: &'static str,
}

fn strings(locale: Locale) -> Strings {
    match locale {
        Locale::En => Strings {
            subject_prefix: "[Gyre]",
            view_button: "View in Gyre",
            priority_label: "Priority",
            time_label: "Time",
            digest_heading: "Your Gyre digest",
            digest_intro: "Notifications received since your last digest:",
            unsubscribe: "Unsubscribe from emails like this",
            unsubscribe_note: "You can change email settings in Gyre under Settings → Notifications.",
            priority_urgent: "Urgent",
            priority_high: "High",
            priority_medium: "Medium",
            priority_low: "Low",
        },
        Locale::De => Strings {
            subject_prefix: "[Gyre]",
            view_button: "In Gyre öffnen",
            priority_label: "Priorität",
            time_label: "Zeitpunkt",
            digest_heading: "Ihr Gyre-Digest",
            digest_intro: "Benachrichtigungen seit Ihrem letzten Digest:",
            unsubscribe: "E-Mails dieser Art abbestellen",
            unsubscribe_note: "Sie können E-Mail-Einstellungen in Gyre unter Einstellungen → Benachrichtigungen ändern.",
            priority_urgent: "Dringend",
            priority_high: "Hoch",
            priority_medium: "Mittel",
            priority_low: "Niedrig",
        },
    }
}

/// Per-type email copy: a short heading naming the event class. The match is
/// exhaustive over `NotificationType`, so adding a variant without copy is a
/// compile error — "Email templates per notification type" cannot regress
/// silently.
fn type_heading(nt: &NotificationType, locale: Locale) -> &'static str {
    match locale {
        Locale::En => match nt {
            NotificationType::SpecApprovalRequested => "Spec awaiting approval",
            NotificationType::PersonaApprovalRequested => "Persona awaiting approval",
            NotificationType::AgentEscalation => "Agent escalation",
            NotificationType::AgentBudgetWarning => "Agent budget warning",
            NotificationType::AgentBudgetExhausted => "Agent budget exhausted",
            NotificationType::AgentFailed => "Agent failed",
            NotificationType::GateFailure => "Gate failure",
            NotificationType::GatePassed => "Gates passed",
            NotificationType::MrMerged => "Merge request merged",
            NotificationType::MrReverted => "Merge request reverted",
            NotificationType::MergeQueuePaused => "Merge queue paused",
            NotificationType::BreakingChangeDetected => "Breaking change detected",
            NotificationType::SpecDriftDetected => "Spec drift detected",
            NotificationType::DependencyStale => "Dependency is stale",
            NotificationType::SecurityFinding => "Security finding",
            NotificationType::InvitationReceived => "Workspace invitation",
            NotificationType::MembershipChanged => "Membership changed",
            NotificationType::SystemAlert => "System alert",
            NotificationType::AgentNeedsClarification => "Agent needs clarification",
            NotificationType::CrossWorkspaceSpecChange => "Cross-workspace spec changed",
            NotificationType::ConflictingInterpretations => "Conflicting interpretations",
            NotificationType::MetaSpecDrift => "Meta-spec drift",
            NotificationType::TrustSuggestion => "Trust suggestion",
            NotificationType::SpecAssertionFailure => "Spec assertion failed",
            NotificationType::AbandonedBranch => "Branch abandoned",
            NotificationType::AgentCompleted => "Agent completed",
            NotificationType::SuggestedSpecLink => "Suggested spec link",
            NotificationType::SpecRejected => "Spec rejected",
            NotificationType::ConstraintViolation => "Constraint violation",
            NotificationType::CascadeTestTriggered => "Cascade test triggered",
            NotificationType::CascadeTestFailed => "Cascade test failed",
            NotificationType::DependencyChainTooDeep => "Dependency chain too deep",
            NotificationType::AtomicGroupFailure => "Atomic group failure",
            NotificationType::SpecConflict => "Spec conflict",
            NotificationType::MergeQueueEscalation => "Merge queue escalation",
            NotificationType::ComputeTargetUnavailable => "Compute target unavailable",
            NotificationType::ImpersonationRequest => "Impersonation requested",
            NotificationType::ImpersonationAlert => "Impersonation active",
            NotificationType::SecretExpiring => "Secret expiring",
            NotificationType::PersonaChangePendingApproval => "Persona change awaiting approval",
            NotificationType::WorkspaceOwnerless => "Workspace has no owner",
        },
        Locale::De => match nt {
            NotificationType::SpecApprovalRequested => "Spec wartet auf Freigabe",
            NotificationType::PersonaApprovalRequested => "Persona wartet auf Freigabe",
            NotificationType::AgentEscalation => "Agent-Eskalation",
            NotificationType::AgentBudgetWarning => "Agent-Budget-Warnung",
            NotificationType::AgentBudgetExhausted => "Agent-Budget aufgebraucht",
            NotificationType::AgentFailed => "Agent fehlgeschlagen",
            NotificationType::GateFailure => "Gate fehlgeschlagen",
            NotificationType::GatePassed => "Gates bestanden",
            NotificationType::MrMerged => "Merge-Request gemergt",
            NotificationType::MrReverted => "Merge-Request zurückgesetzt",
            NotificationType::MergeQueuePaused => "Merge-Queue pausiert",
            NotificationType::BreakingChangeDetected => "Breaking Change erkannt",
            NotificationType::SpecDriftDetected => "Spec-Abweichung erkannt",
            NotificationType::DependencyStale => "Abhängigkeit veraltet",
            NotificationType::SecurityFinding => "Sicherheitsbefund",
            NotificationType::InvitationReceived => "Workspace-Einladung",
            NotificationType::MembershipChanged => "Mitgliedschaft geändert",
            NotificationType::SystemAlert => "Systemhinweis",
            NotificationType::AgentNeedsClarification => "Agent benötigt Klärung",
            NotificationType::CrossWorkspaceSpecChange => {
                "Spezifikation über Workspaces hinweg geändert"
            }
            NotificationType::ConflictingInterpretations => "Widersprüchliche Interpretationen",
            NotificationType::MetaSpecDrift => "Meta-Spec-Abweichung",
            NotificationType::TrustSuggestion => "Vertrauensvorschlag",
            NotificationType::SpecAssertionFailure => "Spec-Assertion fehlgeschlagen",
            NotificationType::AbandonedBranch => "Branch aufgegeben",
            NotificationType::AgentCompleted => "Agent abgeschlossen",
            NotificationType::SuggestedSpecLink => "Vorgeschlagener Spec-Link",
            NotificationType::SpecRejected => "Spec abgelehnt",
            NotificationType::ConstraintViolation => "Constraint-Verletzung",
            NotificationType::CascadeTestTriggered => "Kaskadentest ausgelöst",
            NotificationType::CascadeTestFailed => "Kaskadentest fehlgeschlagen",
            NotificationType::DependencyChainTooDeep => "Abhängigkeitskette zu tief",
            NotificationType::AtomicGroupFailure => "Atomic-Group-Fehler",
            NotificationType::SpecConflict => "Spec-Konflikt",
            NotificationType::MergeQueueEscalation => "Merge-Queue-Eskalation",
            NotificationType::ComputeTargetUnavailable => "Compute-Target nicht verfügbar",
            NotificationType::ImpersonationRequest => "Identitätswechsel angefragt",
            NotificationType::ImpersonationAlert => "Identitätswechsel aktiv",
            NotificationType::SecretExpiring => "Secret läuft ab",
            NotificationType::PersonaChangePendingApproval => {
                "Persona-Änderung wartet auf Freigabe"
            }
            NotificationType::WorkspaceOwnerless => "Workspace ohne Owner",
        },
    }
}

fn priority_label(priority: NotificationPriority, s: &Strings) -> &'static str {
    match priority {
        NotificationPriority::Urgent => s.priority_urgent,
        NotificationPriority::High => s.priority_high,
        NotificationPriority::Medium => s.priority_medium,
        NotificationPriority::Low => s.priority_low,
    }
}

// ─── HTML helpers ────────────────────────────────────────────────────────────

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Format a unix-seconds timestamp as a stable UTC string (email clients
/// render it verbatim; the app UI does relative time itself).
fn format_ts(ts: i64) -> String {
    // Days-since-epoch → Y-M-D civil algorithm (Howard Hinnant's), then
    // H:M:S. No external time dependency needed for a fixed format.
    let days = ts.div_euclid(86_400);
    let secs_of_day = ts.rem_euclid(86_400);
    let (h, m, s) = (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    );
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };
    format!("{y:04}-{mth:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

/// Per-type unsubscribe link (§Email Notifications). Points at the
/// authenticated preferences page, which applies the opt-out for exactly
/// this notification type.
pub fn unsubscribe_url(base_url: &str, nt: &NotificationType) -> String {
    format!(
        "{}/settings/notifications?unsubscribe={}",
        base_url.trim_end_matches('/'),
        nt.as_str()
    )
}

/// Absolute deep link for an notification action_url (server-internal path).
fn action_link(base_url: &str, notif: &Notification) -> Option<String> {
    notif
        .action_url
        .as_deref()
        .map(|p| format!("{}{}", base_url.trim_end_matches('/'), p))
}

const HTML_SHELL_START: &str = r#"<!DOCTYPE html>
<html><body style="margin:0;padding:0;background:#f4f5f7;font-family:-apple-system,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0"><tr><td align="center" style="padding:24px;">
<table role="presentation" width="560" cellpadding="0" cellspacing="0" style="background:#ffffff;border-radius:8px;border:1px solid #e2e4e9;">"#;
const HTML_SHELL_END: &str = r#"</table></td></tr></table></body></html>"#;

fn html_footer(base_url: &str, notif: &Notification, s: &Strings) -> String {
    let unsub = unsubscribe_url(base_url, &notif.notification_type);
    format!(
        r#"<tr><td style="padding:16px 24px;border-top:1px solid #e2e4e9;font-size:12px;color:#6b7280;">
<a href="{url}" style="color:#4f46e5;">{label}</a><br/>{note}
</td></tr>"#,
        url = html_escape(&unsub),
        label = html_escape(s.unsubscribe),
        note = html_escape(s.unsubscribe_note),
    )
}

/// Render a single-notification email (per-type template, §Email
/// Notifications "Email templates per notification type"). HTML body plus
/// plain-text fallback.
pub fn render_notification_email(
    notif: &Notification,
    locale: Locale,
    base_url: &str,
) -> RenderedEmail {
    let s = strings(locale);
    let heading = type_heading(&notif.notification_type, locale);
    let priority = NotificationPriority::for_notification_type(&notif.notification_type);
    let subject = format!("{} {}: {}", s.subject_prefix, heading, notif.title);
    let ts = format_ts(notif.created_at);
    let link = action_link(base_url, notif);

    let mut html = String::new();
    html.push_str(HTML_SHELL_START);
    html.push_str(&format!(
        r#"<tr><td style="padding:24px 24px 8px;"><div style="font-size:11px;letter-spacing:0.08em;text-transform:uppercase;color:#6b7280;">{heading}</div>
<h1 style="margin:4px 0 12px;font-size:20px;color:#111827;">{title}</h1></td></tr>"#,
        heading = html_escape(heading),
        title = html_escape(&notif.title),
    ));
    html.push_str(&format!(
        r#"<tr><td style="padding:0 24px;"><p style="margin:0 0 12px;font-size:14px;color:#374151;white-space:pre-wrap;">{body}</p></td></tr>"#,
        body = html_escape(notif.body.as_deref().unwrap_or("")),
    ));
    html.push_str(&format!(
        r#"<tr><td style="padding:0 24px 16px;font-size:12px;color:#6b7280;">{prio_label}: <strong>{prio}</strong> &middot; {time_label}: {ts}</td></tr>"#,
        prio_label = html_escape(s.priority_label),
        prio = html_escape(priority_label(priority, &s)),
        time_label = html_escape(s.time_label),
        ts = html_escape(&ts),
    ));
    if let Some(url) = &link {
        html.push_str(&format!(
            r#"<tr><td style="padding:0 24px 24px;"><a href="{url}" style="display:inline-block;padding:8px 16px;background:#4f46e5;color:#ffffff;border-radius:6px;text-decoration:none;font-size:14px;">{label}</a></td></tr>"#,
            url = html_escape(url),
            label = html_escape(s.view_button),
        ));
    }
    html.push_str(&html_footer(base_url, notif, &s));
    html.push_str(HTML_SHELL_END);

    let mut text = String::new();
    text.push_str(&notif.title);
    text.push_str("\n\n");
    text.push_str(heading);
    text.push('\n');
    if let Some(body) = &notif.body {
        text.push_str(body);
        text.push_str("\n\n");
    }
    text.push_str(&format!(
        "{}: {} / {}: {}\n",
        s.priority_label,
        priority_label(priority, &s),
        s.time_label,
        ts
    ));
    if let Some(url) = &link {
        text.push_str(&format!("{}: {}\n", s.view_button, url));
    }
    text.push_str(&format!(
        "{}: {}\n{}\n",
        s.unsubscribe,
        unsubscribe_url(base_url, &notif.notification_type),
        s.unsubscribe_note
    ));

    RenderedEmail {
        subject,
        html,
        text,
    }
}

/// Render an aggregate digest email (§Email Notifications: "Digest mode
/// aggregates notifications into a single email at configured frequency").
/// Items are grouped by priority band, Urgent first.
pub fn render_digest_email(
    notifs: &[Notification],
    locale: Locale,
    base_url: &str,
) -> RenderedEmail {
    let s = strings(locale);
    let subject = format!(
        "{} {} ({})",
        s.subject_prefix,
        s.digest_heading,
        notifs.len()
    );

    // Group by priority band, Urgent → Low.
    let order = [
        NotificationPriority::Urgent,
        NotificationPriority::High,
        NotificationPriority::Medium,
        NotificationPriority::Low,
    ];
    let mut html = String::new();
    html.push_str(HTML_SHELL_START);
    html.push_str(&format!(
        r#"<tr><td style="padding:24px 24px 8px;"><h1 style="margin:0 0 8px;font-size:20px;color:#111827;">{heading}</h1>
<p style="margin:0 0 12px;font-size:14px;color:#374151;">{intro}</p></td></tr>"#,
        heading = html_escape(s.digest_heading),
        intro = html_escape(s.digest_intro),
    ));
    let mut text = format!("{}\n{}\n\n", s.digest_heading, s.digest_intro);

    for band in order {
        let items: Vec<&Notification> = notifs
            .iter()
            .filter(|n| NotificationPriority::for_notification_type(&n.notification_type) == band)
            .collect();
        if items.is_empty() {
            continue;
        }
        html.push_str(&format!(
            r#"<tr><td style="padding:8px 24px 4px;font-size:12px;letter-spacing:0.08em;text-transform:uppercase;color:#6b7280;">{band}</td></tr>"#,
            band = html_escape(priority_label(band, &s)),
        ));
        text.push_str(&format!("{}\n", priority_label(band, &s).to_uppercase()));
        for n in items {
            let heading = type_heading(&n.notification_type, locale);
            let link = action_link(base_url, n);
            let title_html = match &link {
                Some(url) => format!(
                    r#"<a href="{url}" style="color:#111827;text-decoration:none;">{title}</a>"#,
                    url = html_escape(url),
                    title = html_escape(&n.title),
                ),
                None => html_escape(&n.title),
            };
            html.push_str(&format!(
                r#"<tr><td style="padding:4px 24px;font-size:14px;color:#111827;border-left:3px solid {color};">
{heading} &mdash; {title}<br/><span style="font-size:12px;color:#6b7280;">{ts} &middot; <a href="{unsub}" style="color:#4f46e5;">{unsub_label}</a></span></td></tr>"#,
                heading = html_escape(heading),
                title = title_html,
                ts = html_escape(&format_ts(n.created_at)),
                color = match band {
                    NotificationPriority::Urgent => "#dc2626",
                    NotificationPriority::High => "#ea580c",
                    NotificationPriority::Medium => "#4f46e5",
                    NotificationPriority::Low => "#9ca3af",
                },
                unsub = html_escape(&unsubscribe_url(base_url, &n.notification_type)),
                unsub_label = html_escape(s.unsubscribe),
            ));
            text.push_str(&format!(
                "- {} [{}] — {}\n  {}: {}\n",
                n.title,
                heading,
                format_ts(n.created_at),
                s.unsubscribe,
                unsubscribe_url(base_url, &n.notification_type)
            ));
        }
        text.push('\n');
    }
    // Footer: general unsubscribe pointer (the per-type links are inline).
    html.push_str(&format!(
        r#"<tr><td style="padding:16px 24px;border-top:1px solid #e2e4e9;font-size:12px;color:#6b7280;">{note}</td></tr>"#,
        note = html_escape(s.unsubscribe_note),
    ));
    html.push_str(HTML_SHELL_END);
    text.push_str(s.unsubscribe_note);
    text.push('\n');

    RenderedEmail {
        subject,
        html,
        text,
    }
}

// ─── SMTP client (minimal RFC 5321 against a deployment relay) ───────────────

/// SMTP relay configuration from the environment:
/// `GYRE_SMTP_HOST` (required to enable sending), `GYRE_SMTP_PORT`
/// (default 25), `GYRE_SMTP_FROM` (default `gyre@localhost`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub from: String,
}

/// Read the relay config. Returns `None` when `GYRE_SMTP_HOST` is unset or
/// empty — the digest drain then holds the durable outbox (nothing is sent
/// or dropped until an operator configures a relay).
pub fn smtp_config_from_env() -> Option<SmtpConfig> {
    let host = std::env::var("GYRE_SMTP_HOST").unwrap_or_default();
    if host.trim().is_empty() {
        return None;
    }
    let port = std::env::var("GYRE_SMTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(25);
    let from = std::env::var("GYRE_SMTP_FROM").unwrap_or_else(|_| "gyre@localhost".to_string());
    Some(SmtpConfig { host, port, from })
}

/// RFC 2047 encoded-word for non-ASCII subjects (`=?UTF-8?B?...?=`).
fn encode_subject(subject: &str) -> String {
    if subject.is_ascii() {
        subject.to_string()
    } else {
        format!("=?UTF-8?B?{}?=", B64.encode(subject.as_bytes()))
    }
}

/// Wrap base64 payload at 76 chars per RFC 2045, CRLF line endings.
fn wrap_base64(data: &[u8]) -> String {
    let encoded = B64.encode(data);
    let mut out = String::with_capacity(encoded.len() + encoded.len() / 76 * 2 + 2);
    for (i, c) in encoded.chars().enumerate() {
        if i > 0 && i % 76 == 0 {
            out.push_str("\r\n");
        }
        out.push(c);
    }
    out
}

/// Build the full RFC 5322 message (multipart/alternative: text + html).
fn build_message(cfg: &SmtpConfig, to: &str, email: &RenderedEmail) -> String {
    let mut msg = String::new();
    msg.push_str(&format!("From: {}\r\n", cfg.from));
    msg.push_str(&format!("To: {}\r\n", to));
    msg.push_str(&format!("Subject: {}\r\n", encode_subject(&email.subject)));
    msg.push_str("MIME-Version: 1.0\r\n");
    msg.push_str("Content-Type: multipart/alternative; boundary=\"=_gyre_mail\"\r\n\r\n");
    msg.push_str("--=_gyre_mail\r\n");
    msg.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    msg.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    msg.push_str(&wrap_base64(email.text.as_bytes()));
    msg.push_str("\r\n--=_gyre_mail\r\n");
    msg.push_str("Content-Type: text/html; charset=utf-8\r\n");
    msg.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    msg.push_str(&wrap_base64(email.html.as_bytes()));
    msg.push_str("\r\n--=_gyre_mail--\r\n");
    msg
}

/// Dot-stuff a DATA payload per RFC 5321 §4.5.2 (a line starting with '.'
/// is transmitted as '..').
fn dot_stuff(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for line in body.split("\r\n") {
        if line.starts_with('.') {
            out.push('.');
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out
}

/// Read one SMTP reply (possibly multi-line `250-...` continuation) and
/// return its 3-digit code.
async fn read_reply<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> anyhow::Result<u16> {
    loop {
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(SMTP_TIMEOUT_SECS),
            reader.read_line(&mut line),
        )
        .await??;
        let line = line.trim_end();
        let bytes = line.as_bytes();
        if bytes.len() < 4 {
            anyhow::bail!("short SMTP reply: {line:?}");
        }
        // Parse the 3-digit code from raw bytes. Never slice the &str at a
        // fixed byte index: a reply line whose byte 3 falls inside a
        // multibyte UTF-8 char (e.g. "üü\r\n" — 4 bytes, passing the length
        // guard) would panic on a non-char-boundary, unwinding through
        // send_email → run_digest_once → the spawned email_digest job loop
        // and killing the drain until restart (task-113 review finding;
        // same class as the task-095 byte-slice invariant).
        let code: u16 = std::str::from_utf8(&bytes[..3])
            .map_err(|_| anyhow::anyhow!("malformed SMTP reply code (non-ASCII): {line:?}"))?
            .parse()
            .map_err(|_| anyhow::anyhow!("malformed SMTP reply code: {line:?}"))?;
        // "250-..." = continuation; "250 ..." = final line of the reply.
        if bytes[3] == b' ' {
            return Ok(code);
        }
        if bytes[3] != b'-' {
            anyhow::bail!("malformed SMTP reply: {line:?}");
        }
    }
}

async fn write_cmd(writer: &mut tokio::net::tcp::OwnedWriteHalf, cmd: &str) -> anyhow::Result<()> {
    tokio::time::timeout(
        std::time::Duration::from_secs(SMTP_TIMEOUT_SECS),
        writer.write_all(format!("{cmd}\r\n").as_bytes()),
    )
    .await??;
    Ok(())
}

/// Send one email through the configured relay. Minimal but real RFC 5321:
/// greeting → EHLO → MAIL FROM → RCPT TO → DATA (dot-stuffed, CRLF) → QUIT.
pub async fn send_email(cfg: &SmtpConfig, to: &str, email: &RenderedEmail) -> anyhow::Result<()> {
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(SMTP_TIMEOUT_SECS),
        TcpStream::connect((cfg.host.as_str(), cfg.port)),
    )
    .await??;
    stream.set_nodelay(true)?;
    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut writer = write_half;

    let expect = |want: u16, got: u16, phase: &str| -> anyhow::Result<()> {
        if got != want {
            anyhow::bail!("SMTP relay rejected {phase}: got {got}, expected {want}");
        }
        Ok(())
    };

    let code = read_reply(&mut reader).await?;
    expect(220, code, "connection greeting")?;
    write_cmd(&mut writer, "EHLO gyre.local").await?;
    let code = read_reply(&mut reader).await?;
    expect(250, code, "EHLO")?;
    write_cmd(&mut writer, &format!("MAIL FROM:<{}>", cfg.from)).await?;
    let code = read_reply(&mut reader).await?;
    expect(250, code, "MAIL FROM")?;
    write_cmd(&mut writer, &format!("RCPT TO:<{to}>")).await?;
    let code = read_reply(&mut reader).await?;
    expect(250, code, "RCPT TO")?;
    write_cmd(&mut writer, "DATA").await?;
    let code = read_reply(&mut reader).await?;
    expect(354, code, "DATA")?;

    let payload = dot_stuff(&build_message(cfg, to, email));
    tokio::time::timeout(
        std::time::Duration::from_secs(SMTP_TIMEOUT_SECS),
        writer.write_all(payload.as_bytes()),
    )
    .await??;
    write_cmd(&mut writer, ".").await?;
    let code = read_reply(&mut reader).await?;
    expect(250, code, "message body")?;
    let _ = write_cmd(&mut writer, "QUIT").await;
    Ok(())
}

// ─── Digest drain (§Email Notifications: digest mode + priority filter) ──────

/// One parsed outbox entry.
struct OutboxEntry {
    key: String,
    notif: Notification,
}

/// Parse an outbox kv row. Prefers the embedded `notification` field
/// (task-113+); falls back to reconstructing from the task-112 flat shape so
/// entries queued by a pre-digest deployment still deliver.
fn parse_outbox_entry(key: String, raw: &str) -> Option<OutboxEntry> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if let Some(n) = v.get("notification") {
        let notif: Notification = serde_json::from_value(n.clone()).ok()?;
        return Some(OutboxEntry { key, notif });
    }
    // Flat task-112 shape.
    let get_str = |field: &str| v.get(field).and_then(|x| x.as_str()).map(|s| s.to_string());
    let id = get_str("notification_id")?;
    let user_id = get_str("user_id")?;
    let workspace_id = get_str("workspace_id")?;
    let type_str = get_str("notification_type")?;
    let notification_type = NotificationType::parse(&type_str)?;
    let subject = get_str("subject").unwrap_or_default();
    let title = subject
        .strip_prefix("[Gyre] ")
        .unwrap_or(&subject)
        .to_string();
    let created_at = v.get("queued_at").and_then(|x| x.as_i64()).unwrap_or(0);
    let tenant_id = get_str("tenant_id").unwrap_or_default();
    let mut notif = Notification::new(
        Id::new(id),
        Id::new(workspace_id),
        Id::new(user_id),
        notification_type,
        title,
        tenant_id,
        created_at,
    );
    notif.body = get_str("body");
    notif.action_url = get_str("action_url");
    notif.priority_rank = v.get("priority").and_then(|x| x.as_i64()).unwrap_or(5) as u8;
    Some(OutboxEntry { key, notif })
}

/// True when the entry still passes the user's CURRENT email gating
/// (enabled, digest on, priority band ≥ min_priority, type not unsubscribed).
fn email_gated_in(notif: &Notification, channels: &gyre_domain::NotificationChannels) -> bool {
    if !channels.email.enabled || channels.email.digest == DigestFrequency::Off {
        return false;
    }
    let priority = NotificationPriority::for_notification_type(&notif.notification_type);
    if priority < channels.email.min_priority {
        return false;
    }
    if channels
        .email
        .type_overrides
        .get(notif.notification_type.as_str())
        == Some(&true)
    {
        return false;
    }
    true
}

/// Seconds between digest sends per frequency.
fn digest_window_secs(freq: DigestFrequency) -> u64 {
    match freq {
        DigestFrequency::Hourly => 3600,
        DigestFrequency::Daily => 86_400,
        DigestFrequency::Immediate | DigestFrequency::Off => 0,
    }
}

/// Drain the durable email outbox (user-management.md §Email Notifications).
///
/// Per user: re-checks the current channel gating (dropping entries the user
/// has since unsubscribed from / deprioritized), then delivers at the
/// configured frequency — Immediate sends each entry as its own email,
/// Hourly/Daily aggregate every pending entry into ONE digest email once the
/// frequency window since the last digest has elapsed. Sent entries are
/// removed from the outbox; failed sends keep their entries for the next
/// cycle (durable retry).
///
/// When no SMTP relay is configured (`GYRE_SMTP_HOST` unset) the queue is
/// held untouched — queue-and-hold, nothing silently dropped.
pub async fn run_digest_once(state: &AppState) -> anyhow::Result<()> {
    let Some(smtp) = smtp_config_from_env() else {
        tracing::debug!("email digest: GYRE_SMTP_HOST not configured — holding outbox entries");
        return Ok(());
    };
    let rows = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await?;
    if rows.is_empty() {
        return Ok(());
    }

    let mut by_user: HashMap<String, Vec<OutboxEntry>> = HashMap::new();
    for (key, raw) in &rows {
        match parse_outbox_entry(key.clone(), raw) {
            Some(entry) => by_user
                .entry(entry.notif.user_id.as_str().to_string())
                .or_default()
                .push(entry),
            None => {
                tracing::warn!(key, "email digest: unparseable outbox entry dropped");
                let _ = state.kv_store.kv_remove(EMAIL_OUTBOX_NS, key).await;
            }
        }
    }

    let now = crate::api::now_secs();
    for (user_str, entries) in by_user {
        let user_id = Id::new(user_str.clone());
        let user = match state.users.find_by_id(&user_id).await? {
            Some(u) => u,
            None => {
                tracing::warn!(user_id = %user_id, "email digest: outbox user no longer exists");
                for e in &entries {
                    let _ = state.kv_store.kv_remove(EMAIL_OUTBOX_NS, &e.key).await;
                }
                continue;
            }
        };
        let Some(to) = user.email.clone().filter(|e| !e.trim().is_empty()) else {
            tracing::warn!(
                user_id = %user_id,
                "email digest: user has email channel enabled but no address — dropping queued entries"
            );
            for e in &entries {
                let _ = state.kv_store.kv_remove(EMAIL_OUTBOX_NS, &e.key).await;
            }
            continue;
        };
        let channels = resolve_channels(state, &user_id).await;

        // Re-check gating against the CURRENT config: entries queued under an
        // older config that the user has since tightened are dropped, not sent.
        let mut sendable: Vec<Notification> = Vec::new();
        let mut sendable_keys: Vec<String> = Vec::new();
        for entry in entries {
            if email_gated_in(&entry.notif, &channels) {
                sendable_keys.push(entry.key);
                sendable.push(entry.notif);
            } else {
                let _ = state.kv_store.kv_remove(EMAIL_OUTBOX_NS, &entry.key).await;
            }
        }
        if sendable.is_empty() {
            continue;
        }

        match channels.email.digest {
            DigestFrequency::Immediate => {
                for (notif, key) in sendable.iter().zip(&sendable_keys) {
                    let email = render_notification_email(
                        notif,
                        resolve_locale(&user.locale),
                        &state.base_url,
                    );
                    if let Err(e) = send_email(&smtp, &to, &email).await {
                        tracing::warn!(user_id = %user_id, error = %e, "email digest: immediate send failed — entry kept for retry");
                        continue;
                    }
                    let _ = state.kv_store.kv_remove(EMAIL_OUTBOX_NS, key).await;
                }
            }
            DigestFrequency::Hourly | DigestFrequency::Daily => {
                let window = digest_window_secs(channels.email.digest);
                let last_sent = state
                    .kv_store
                    .kv_get(DIGEST_STATE_NS, &user_str)
                    .await?
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                if now.saturating_sub(last_sent) < window {
                    continue; // Window not elapsed — hold entries for the next due digest.
                }
                let email =
                    render_digest_email(&sendable, resolve_locale(&user.locale), &state.base_url);
                match send_email(&smtp, &to, &email).await {
                    Ok(()) => {
                        for key in &sendable_keys {
                            let _ = state.kv_store.kv_remove(EMAIL_OUTBOX_NS, key).await;
                        }
                        let _ = state
                            .kv_store
                            .kv_set(DIGEST_STATE_NS, &user_str, now.to_string())
                            .await;
                    }
                    Err(e) => {
                        tracing::warn!(user_id = %user_id, error = %e, "email digest: digest send failed — entries kept for retry");
                    }
                }
            }
            DigestFrequency::Off => {
                // email_gated_in already dropped everything (digest Off).
            }
        }
    }
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_notif(nt: NotificationType, title: &str, created_at: i64) -> Notification {
        let mut n = Notification::new(
            Id::new("n-1"),
            Id::new("ws-1"),
            Id::new("user-1"),
            nt,
            title.to_string(),
            "t-1",
            created_at,
        );
        n.body = Some("Body text".to_string());
        n.action_url = Some("/workspaces/acme".to_string());
        n
    }

    /// Every notification type renders — the exhaustive `type_heading`
    /// match already enforces coverage at compile time; this asserts the
    /// rendered output actually carries per-type copy, links, and both
    /// bodies.
    #[test]
    fn single_email_renders_per_type_templates() {
        let notif = make_notif(NotificationType::GateFailure, "lint failed", 1_700_000_000);
        let email = render_notification_email(&notif, Locale::En, "https://gyre.example");
        assert!(email.subject.contains("[Gyre] Gate failure: lint failed"));
        assert!(email.html.contains("Gate failure"));
        assert!(email.html.contains("https://gyre.example/workspaces/acme"));
        assert!(email.html.contains("unsubscribe=GateFailure"));
        assert!(email.text.contains("lint failed"));
        assert!(email
            .text
            .contains("https://gyre.example/settings/notifications?unsubscribe=GateFailure"));
    }

    /// The user's configured locale drives the rendering (§Email
    /// Notifications) — a de-DE user gets German copy, an unknown tag
    /// falls back to English.
    #[test]
    fn locale_rendering() {
        assert_eq!(resolve_locale("de-DE"), Locale::De);
        assert_eq!(resolve_locale("en-US"), Locale::En);
        assert_eq!(resolve_locale("xx-QQ"), Locale::En);

        let notif = make_notif(NotificationType::AgentFailed, "worker-3", 0);
        let de = render_notification_email(&notif, resolve_locale("de-DE"), "https://g");
        assert!(de.subject.contains("Agent fehlgeschlagen"));
        assert!(de.html.contains("E-Mails dieser Art abbestellen"));
        let en = render_notification_email(&notif, resolve_locale("xx-QQ"), "https://g");
        assert!(en.subject.contains("Agent failed"));
    }

    /// The digest aggregates: one subject, items grouped by priority band
    /// with per-item unsubscribe links.
    #[test]
    fn digest_aggregates_and_groups_by_priority() {
        let notifs = vec![
            make_notif(NotificationType::MergeQueuePaused, "queue paused", 100),
            make_notif(NotificationType::AgentFailed, "agent down", 200),
            make_notif(NotificationType::SuggestedSpecLink, "maybe link", 300),
        ];
        let email = render_digest_email(&notifs, Locale::En, "https://gyre.example");
        assert!(email.subject.contains("3"));
        // Urgent/High section precedes Medium in the HTML.
        let high = email
            .html
            .find("HIGH")
            .or_else(|| email.html.find("High"))
            .expect("high band");
        let medium = email
            .html
            .find("MEDIUM")
            .or_else(|| email.html.find("Medium"))
            .expect("medium band");
        assert!(high < medium);
        assert!(email.html.matches("unsubscribe=").count() >= 3);
        assert!(email.text.contains("agent down"));
    }

    #[test]
    fn subject_encoding_and_dot_stuffing() {
        assert_eq!(encode_subject("plain"), "plain");
        let enc = encode_subject("Ihr Gyre-Digest (3)");
        assert!(enc.starts_with("=?UTF-8?B?"));
        let stuffed = dot_stuff("line one\r\n.hidden\r\n");
        assert_eq!(stuffed, "line one\r\n..hidden\r\n");
    }

    /// Gating: a per-type unsubscribe drops the entry even when the global
    /// email config would otherwise deliver it; min_priority drops
    /// lower-band entries.
    #[test]
    fn email_gating_respects_type_overrides_and_priority() {
        let mut channels = gyre_domain::NotificationChannels::default();
        channels.email.enabled = true;
        channels.email.digest = DigestFrequency::Daily;
        channels.email.min_priority = NotificationPriority::Medium;

        let low = make_notif(NotificationType::SuggestedSpecLink, "sugg", 0); // Low band
        assert!(!email_gated_in(&low, &channels), "below min_priority");

        let high = make_notif(NotificationType::AgentFailed, "fail", 0); // High band
        assert!(email_gated_in(&high, &channels));

        channels
            .email
            .type_overrides
            .insert("AgentFailed".to_string(), true);
        assert!(
            !email_gated_in(&high, &channels),
            "per-type unsubscribe must drop the entry"
        );

        channels.email.enabled = false;
        assert!(!email_gated_in(&high, &channels));
    }

    /// Outbox entries round-trip: embedded notification preferred, flat
    /// task-112 shape still parses.
    #[test]
    fn outbox_entry_parsing() {
        let notif = make_notif(NotificationType::GatePassed, "green", 42);
        let embedded = serde_json::json!({
            "notification_id": "n-1",
            "user_id": "user-1",
            "subject": "[Gyre] green",
            "notification": serde_json::to_value(&notif).unwrap(),
        });
        let e = parse_outbox_entry("k".into(), &embedded.to_string()).unwrap();
        assert_eq!(e.notif.title, "green");
        assert_eq!(e.notif.notification_type, NotificationType::GatePassed);

        let flat = serde_json::json!({
            "notification_id": "n-2",
            "user_id": "user-1",
            "workspace_id": "ws-1",
            "subject": "[Gyre] queue paused",
            "notification_type": "MergeQueuePaused",
            "priority": 3,
            "body": "broken main",
            "action_url": "/workspaces/acme",
            "queued_at": 7,
        });
        let e = parse_outbox_entry("k2".into(), &flat.to_string()).unwrap();
        assert_eq!(e.notif.title, "queue paused");
        assert_eq!(e.notif.priority_rank, 3);
        assert_eq!(e.notif.action_url.as_deref(), Some("/workspaces/acme"));
    }

    /// `read_reply` must never panic on a reply line whose byte 3 falls
    /// inside a multibyte UTF-8 char (task-113 review finding: the old
    /// `line[..3]` slice killed the spawned email_digest job on panic —
    /// "üü\r\n" is 4 bytes after trim_end, passing the length guard).
    /// It must return an error instead, keeping the drain loop alive.
    #[test]
    fn read_reply_non_char_boundary_line_errors_not_panics() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (client, mut server) = tokio::io::duplex(64);
            let writer = tokio::spawn(async move {
                server
                    .write_all("\u{fc}\u{fc}\r\n".as_bytes())
                    .await
                    .unwrap();
            });
            let mut reader = BufReader::new(client);
            let result = read_reply(&mut reader).await;
            writer.await.unwrap();
            let err = result.expect_err("non-ASCII reply must error, not panic");
            assert!(
                err.to_string().contains("malformed SMTP reply"),
                "unexpected error: {err:#}"
            );
        });
    }

    /// Positive control: multi-line `250-...` continuation replies are
    /// joined and the final code returned; a short garbage line errors.
    #[test]
    fn read_reply_continuation_and_short_line() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (client, mut server) = tokio::io::duplex(64);
            let writer = tokio::spawn(async move {
                server
                    .write_all(b"250-relay.local\r\n250 8BITMIME\r\n")
                    .await
                    .unwrap();
            });
            let mut reader = BufReader::new(client);
            let code = read_reply(&mut reader).await.expect("valid reply");
            writer.await.unwrap();
            assert_eq!(code, 250);

            let (client, mut server) = tokio::io::duplex(64);
            let writer = tokio::spawn(async move {
                server.write_all(b"250\r\n").await.unwrap();
            });
            let mut reader = BufReader::new(client);
            let result = read_reply(&mut reader).await;
            writer.await.unwrap();
            assert!(result.is_err(), "3-byte line has no separator byte");
        });
    }

    /// End-to-end digest drain against a real in-process SMTP listener:
    /// a Daily user with a window long elapsed gets ONE aggregated email
    /// carrying every queued entry; entries are removed from the outbox and
    /// the per-user last-sent stamp is written. Requires GYRE_SMTP_HOST —
    /// set via a lock-serialized env mutation.
    #[test]
    fn digest_drain_sends_aggregate_and_clears_outbox() {
        // Serialize env-mutating tests (Rust runs tests in parallel threads).
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            // Fake relay: accept one connection, record the DATA payload.
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();

            let relay = tokio::spawn(async move {
                let (mut sock, _) = listener.accept().await.unwrap();
                let (r, mut w) = sock.split();
                let mut reader = BufReader::new(r);
                let mut payload = String::new();
                let mut in_data = false;
                w.write_all(b"220 relay.local ESMTP\r\n").await.unwrap();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap() == 0 {
                        break;
                    }
                    let cmd = line.trim_end();
                    if in_data {
                        if cmd == "." {
                            in_data = false;
                            w.write_all(b"250 OK queued\r\n").await.unwrap();
                        } else {
                            payload.push_str(&line);
                        }
                    } else {
                        match cmd {
                            c if c.starts_with("EHLO") => {
                                w.write_all(b"250-relay.local\r\n250 8BITMIME\r\n")
                                    .await
                                    .unwrap();
                            }
                            c if c.starts_with("MAIL FROM") => {
                                w.write_all(b"250 OK\r\n").await.unwrap()
                            }
                            c if c.starts_with("RCPT TO") => {
                                w.write_all(b"250 OK\r\n").await.unwrap()
                            }
                            "DATA" => {
                                in_data = true;
                                w.write_all(b"354 go ahead\r\n").await.unwrap();
                            }
                            "QUIT" => {
                                w.write_all(b"221 bye\r\n").await.unwrap();
                                break;
                            }
                            _ => w.write_all(b"500 what\r\n").await.unwrap(),
                        }
                    }
                }
                payload
            });

            std::env::set_var("GYRE_SMTP_HOST", "127.0.0.1");
            std::env::set_var("GYRE_SMTP_PORT", addr.port().to_string());
            std::env::remove_var("GYRE_SMTP_FROM");

            let state = crate::mem::test_state();
            // Seed a user with email enabled (Daily digest, long-elapsed
            // window) and two queued notifications.
            let mut user =
                gyre_domain::User::new(Id::new("user-digest"), "ext-1", "digest-user", 1);
            user.email = Some("digest@example.com".to_string());
            user.locale = "en-US".to_string();
            let mut channels = gyre_domain::NotificationChannels::default();
            channels.email.enabled = true;
            channels.email.digest = DigestFrequency::Daily;
            channels.email.min_priority = NotificationPriority::Medium;
            user.preferences.notification_channels = channels.clone();
            state.users.create(&user).await.unwrap();
            state
                .user_channel_prefs
                .upsert(&user.id, &channels)
                .await
                .unwrap();

            for (i, nt) in [
                NotificationType::AgentFailed,
                NotificationType::MergeQueuePaused,
            ]
            .into_iter()
            .enumerate()
            {
                let mut n = make_notif(nt, "entry", 100 + i as i64);
                n.user_id = user.id.clone();
                state
                    .kv_store
                    .kv_set(
                        EMAIL_OUTBOX_NS,
                        &format!("{}:n-{i}", user.id.as_str()),
                        serde_json::json!({
                            "notification": serde_json::to_value(&n).unwrap(),
                        })
                        .to_string(),
                    )
                    .await
                    .unwrap();
            }
            // Long-elapsed window: last digest sent 2 days ago.
            state
                .kv_store
                .kv_set(DIGEST_STATE_NS, user.id.as_str(), "1".to_string())
                .await
                .unwrap();

            run_digest_once(&state).await.unwrap();

            let payload = relay.await.unwrap();
            assert!(payload.contains("To: digest@example.com"));
            // One aggregate email carrying both entries.
            assert!(
                payload.contains("Your Gyre digest (2)"),
                "subject: {}",
                payload
            );
            // Base64 bodies — decode and check aggregation content.
            assert!(payload.contains("Content-Type: multipart/alternative"));

            let remaining = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
            assert!(
                remaining.is_empty(),
                "outbox must be drained, got {remaining:?}"
            );
            let stamp = state
                .kv_store
                .kv_get(DIGEST_STATE_NS, user.id.as_str())
                .await
                .unwrap();
            assert!(stamp.unwrap_or_default().parse::<u64>().unwrap() > 1);

            std::env::remove_var("GYRE_SMTP_HOST");
            std::env::remove_var("GYRE_SMTP_PORT");

            // Queue-and-hold: with no relay configured, entries stay put.
            let mut n = make_notif(NotificationType::AgentFailed, "held", 1);
            n.user_id = user.id.clone();
            state
                .kv_store
                .kv_set(
                    EMAIL_OUTBOX_NS,
                    format!("{}:n-held", user.id.as_str()).as_str(),
                    serde_json::json!({ "notification": serde_json::to_value(&n).unwrap() })
                        .to_string(),
                )
                .await
                .unwrap();
            run_digest_once(&state).await.unwrap();
            let held = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
            assert_eq!(held.len(), 1, "unconfigured SMTP must hold entries");
        });
    }
}
