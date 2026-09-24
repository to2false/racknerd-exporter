use crate::api::{ApiError, Info, Usage};
use std::fmt::Write;

pub struct Snapshot {
    pub result: Result<Info, ApiError>,
    pub duration_seconds: f64,
    pub collected_at: f64,
    pub last_success: f64,
    pub requests: u64,
    pub failures: u64,
}

pub fn render(server: &str, snapshot: &Snapshot) -> String {
    let labels = format!("server=\"{}\"", escape(server));
    let mut out = String::with_capacity(4096);
    metric(
        &mut out,
        "racknerd_exporter_build_info",
        "Exporter build information.",
        "gauge",
        &format!("version=\"{}\"", env!("CARGO_PKG_VERSION")),
        1,
    );
    metric(
        &mut out,
        "racknerd_up",
        "Whether the most recent API collection succeeded.",
        "gauge",
        &labels,
        u8::from(snapshot.result.is_ok()),
    );
    metric(
        &mut out,
        "racknerd_scrape_duration_seconds",
        "Duration of the most recent API collection.",
        "gauge",
        &labels,
        snapshot.duration_seconds,
    );
    metric(
        &mut out,
        "racknerd_last_collection_timestamp_seconds",
        "Unix timestamp of the most recent API collection, successful or failed.",
        "gauge",
        &labels,
        snapshot.collected_at,
    );
    metric(
        &mut out,
        "racknerd_last_success_timestamp_seconds",
        "Unix timestamp of the last successful API collection, or zero before any success.",
        "gauge",
        &labels,
        snapshot.last_success,
    );
    metric(
        &mut out,
        "racknerd_api_requests_total",
        "Number of API collection attempts since exporter start.",
        "counter",
        &labels,
        snapshot.requests,
    );
    metric(
        &mut out,
        "racknerd_api_failures_total",
        "Number of failed API collections since exporter start.",
        "counter",
        &labels,
        snapshot.failures,
    );
    if let Ok(info) = &snapshot.result {
        metric(
            &mut out,
            "racknerd_vps_state_known",
            "Whether the API returned an online, offline, or disabled VM state.",
            "gauge",
            &labels,
            u8::from(info.online.is_some()),
        );
        if let Some(online) = info.online {
            metric(
                &mut out,
                "racknerd_vps_online",
                "VM state reported by the API; online is 1, offline or disabled is 0.",
                "gauge",
                &labels,
                u8::from(online),
            );
        }
        if let Some(disabled) = info.disabled {
            metric(
                &mut out,
                "racknerd_vps_disabled",
                "Whether the API reports the VM as disabled; unknown states are omitted.",
                "gauge",
                &labels,
                u8::from(disabled),
            );
        }
        out.push_str("# HELP racknerd_resource_available Whether usable statistics were returned for this resource.\n# TYPE racknerd_resource_available gauge\n");
        for (name, value) in [
            ("bandwidth", &info.bandwidth),
            ("memory", &info.memory),
            ("disk", &info.disk),
        ] {
            writeln!(
                &mut out,
                "racknerd_resource_available{{{labels},resource=\"{name}\"}} {}",
                u8::from(value.is_some())
            )
            .unwrap();
        }
        for (name, value) in [
            ("bandwidth", &info.bandwidth),
            ("memory", &info.memory),
            ("disk", &info.disk),
        ] {
            if let Some(value) = value {
                usage(&mut out, &labels, name, value);
            }
        }
    }
    out
}

fn usage(out: &mut String, labels: &str, resource: &str, value: &Usage) {
    for (suffix, help, value) in [
        (
            "limit_bytes",
            "Resource quota or capacity in bytes reported by the provider.",
            value.limit,
        ),
        (
            "used_bytes",
            "Current resource usage in bytes reported by the provider; may reset or decrease.",
            value.used,
        ),
        (
            "remaining_bytes",
            "Remaining resource bytes reported by the provider; may be negative on overage.",
            value.free,
        ),
    ] {
        metric(
            out,
            &format!("racknerd_{resource}_{suffix}"),
            help,
            "gauge",
            labels,
            value,
        );
    }
    if value.limit > 0.0 {
        metric(
            out,
            &format!("racknerd_{resource}_used_ratio"),
            "Used bytes divided by a positive quota; values above 1 indicate overage.",
            "gauge",
            labels,
            value.used / value.limit,
        );
    }
}

fn metric(
    out: &mut String,
    name: &str,
    help: &str,
    kind: &str,
    labels: &str,
    value: impl std::fmt::Display,
) {
    writeln!(
        out,
        "# HELP {name} {help}\n# TYPE {name} {kind}\n{name}{{{labels}}} {value}"
    )
    .unwrap();
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_is_known_and_distinguishable_from_offline() {
        let snapshot = Snapshot {
            result: crate::api::parse_info(
                "<ctrl><status>success</status><vmstat>disabled</vmstat></ctrl>",
                false,
            ),
            duration_seconds: 0.1,
            collected_at: 200.0,
            last_success: 200.0,
            requests: 1,
            failures: 0,
        };
        let output = render("test-vps", &snapshot);
        assert!(output.contains("racknerd_vps_state_known{server=\"test-vps\"} 1"));
        assert!(output.contains("racknerd_vps_online{server=\"test-vps\"} 0"));
        assert!(output.contains("racknerd_vps_disabled{server=\"test-vps\"} 1"));
    }

    #[test]
    fn failure_exposes_health_without_stale_vps_metrics_or_error_body() {
        let snapshot = Snapshot {
            result: Err(ApiError::Rejected),
            duration_seconds: 0.1,
            collected_at: 200.0,
            last_success: 100.0,
            requests: 2,
            failures: 1,
        };
        let output = render("vps\"\\\n", &snapshot);
        assert!(output.contains("server=\"vps\\\"\\\\\\n\""));
        assert!(output.contains("racknerd_up{server=\"vps\\\"\\\\\\n\"} 0"));
        assert!(!output.contains("racknerd_vps_online"));
        assert!(!output.contains("racknerd_bandwidth"));
        assert!(!output.contains("credentials"));
    }
}
