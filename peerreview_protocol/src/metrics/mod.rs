//! Module de métriques pour les tests PeerReview
//!
//! Ce module implémente les 5 métriques obligatoires:
//! 1. Content Delivery Rate - % de chunks reçus par nœud
//! 2. Network Traffic - Bytes/messages envoyés par nœud
//! 3. Detection Time - Temps entre injection faute → détection
//! 4. Propagation Latency - Temps source → dernier nœud
//! 5. Fault Detection Accuracy - Précision (FP, FN, détection rate)

use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::fs::File;
use std::io::Write;

use crate::types::NodeId;

/// Métriques de livraison de contenu (Metric 1)
///
/// Content Delivery Rate = % de nœuds ayant reçu le contenu
/// Selon le PDF: "Pourcentage de chunks reçus par chaque nœud"
/// Note: Le nœud source est exclu du calcul car il est l'émetteur, pas un destinataire
#[derive(Debug, Clone, Default)]
pub struct ContentDeliveryMetrics {
    pub total_chunks_sent: usize,
    pub total_nodes: usize,
    pub source_node: Option<NodeId>,  // Nœud source à exclure du calcul
    pub chunks_received_per_node: HashMap<NodeId, usize>,
    pub delivery_rate_per_node: HashMap<NodeId, f64>,
    pub avg_delivery_rate: f64,
    pub min_delivery_rate: f64,
    pub max_delivery_rate: f64,
}

impl ContentDeliveryMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_total_nodes(&mut self, total: usize) {
        self.total_nodes = total;
    }

    pub fn set_source_node(&mut self, source: NodeId) {
        self.source_node = Some(source);
    }

    pub fn record_send(&mut self) {
        self.total_chunks_sent += 1;
    }

    pub fn record_receive(&mut self, node_id: NodeId) {
        *self.chunks_received_per_node.entry(node_id).or_insert(0) += 1;
    }

    pub fn calculate_rates(&mut self) {
        if self.total_chunks_sent == 0 {
            return;
        }

        // Delivery rate per node: 100% if received at least one chunk, 0% otherwise
        let mut rates = Vec::new();
        for (&node_id, &received) in &self.chunks_received_per_node {
            // Un nœud a reçu le contenu si received >= 1
            let rate = if received >= self.total_chunks_sent { 100.0 } else {
                (received as f64 / self.total_chunks_sent as f64) * 100.0
            };
            // Cap at 100%
            let rate = rate.min(100.0);
            self.delivery_rate_per_node.insert(node_id, rate);
            rates.push(rate);
        }

        // Avg delivery rate = % de nœuds destinataires ayant reçu tous les chunks
        // Le nœud source est exclu car il est l'émetteur, pas un destinataire
        let destination_nodes = if self.source_node.is_some() {
            self.total_nodes.saturating_sub(1)  // Exclure le source
        } else {
            self.total_nodes
        };

        if destination_nodes > 0 {
            let nodes_with_full_delivery = self.chunks_received_per_node
                .values()
                .filter(|&&r| r >= self.total_chunks_sent)
                .count();
            self.avg_delivery_rate = (nodes_with_full_delivery as f64 / destination_nodes as f64) * 100.0;
        } else if !rates.is_empty() {
            self.avg_delivery_rate = rates.iter().sum::<f64>() / rates.len() as f64;
        }

        if !rates.is_empty() {
            self.min_delivery_rate = rates.iter().cloned().fold(f64::INFINITY, f64::min);
            self.max_delivery_rate = rates.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        }
    }
}

/// Statistiques de trafic par nœud
#[derive(Debug, Clone, Default)]
pub struct TrafficStats {
    pub messages_sent: usize,
    pub messages_received: usize,
    pub bytes_sent: usize,
    pub bytes_received: usize,
}

/// Métriques de trafic réseau (Metric 2)
#[derive(Debug, Clone, Default)]
pub struct NetworkTrafficMetrics {
    pub traffic_per_node: HashMap<NodeId, TrafficStats>,
    pub total_messages_sent: usize,
    pub total_bytes_sent: usize,
    pub peerreview_overhead_bytes: usize,
    pub peerreview_overhead_pct: f64,
    pub start_time: Option<Instant>,
    pub duration: Duration,
}

impl NetworkTrafficMetrics {
    pub fn new() -> Self {
        Self {
            start_time: Some(Instant::now()),
            ..Default::default()
        }
    }

    pub fn record_send(&mut self, node_id: NodeId, bytes: usize) {
        let stats = self.traffic_per_node.entry(node_id).or_default();
        stats.messages_sent += 1;
        stats.bytes_sent += bytes;
        self.total_messages_sent += 1;
        self.total_bytes_sent += bytes;
    }

    pub fn record_receive(&mut self, node_id: NodeId, bytes: usize) {
        let stats = self.traffic_per_node.entry(node_id).or_default();
        stats.messages_received += 1;
        stats.bytes_received += bytes;
    }

    pub fn record_pr_overhead(&mut self, bytes: usize) {
        self.peerreview_overhead_bytes += bytes;
    }

    pub fn finalize(&mut self) {
        if let Some(start) = self.start_time {
            self.duration = start.elapsed();
        }
        if self.total_bytes_sent > 0 {
            self.peerreview_overhead_pct =
                (self.peerreview_overhead_bytes as f64 / self.total_bytes_sent as f64) * 100.0;
        }
    }

    pub fn avg_kbps_per_node(&self) -> f64 {
        if self.traffic_per_node.is_empty() || self.duration.as_secs_f64() == 0.0 {
            return 0.0;
        }
        let total_kb = self.total_bytes_sent as f64 / 1024.0;
        total_kb / self.duration.as_secs_f64() / self.traffic_per_node.len() as f64
    }
}

/// Métriques de temps de détection (Metric 3)
#[derive(Debug, Clone)]
pub struct DetectionTimeMetrics {
    pub fault_injection_time: Option<Instant>,
    pub detection_time: Option<Instant>,
    pub time_to_detection_ms: Option<u64>,
    pub detected_by: Vec<NodeId>,
    pub detection_method: DetectionMethod,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectionMethod {
    None,
    Audit,
    Challenge,
    Consistency,
}

impl Default for DetectionTimeMetrics {
    fn default() -> Self {
        Self {
            fault_injection_time: None,
            detection_time: None,
            time_to_detection_ms: None,
            detected_by: Vec::new(),
            detection_method: DetectionMethod::None,
        }
    }
}

impl DetectionTimeMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_fault_injection(&mut self) {
        self.fault_injection_time = Some(Instant::now());
    }

    pub fn record_detection(&mut self, detected_by: NodeId, method: DetectionMethod) {
        if self.detection_time.is_none() {
            self.detection_time = Some(Instant::now());
            if let Some(injection_time) = self.fault_injection_time {
                self.time_to_detection_ms = Some(injection_time.elapsed().as_millis() as u64);
            }
        }
        self.detected_by.push(detected_by);
        self.detection_method = method;
    }
}

/// Métriques de latence de propagation (Metric 4)
#[derive(Debug, Clone, Default)]
pub struct PropagationMetrics {
    pub source_publish_time: Option<Instant>,
    pub node_reception_times: HashMap<NodeId, Instant>,
    pub node_latencies: HashMap<NodeId, Duration>,
    pub min_latency: Duration,
    pub max_latency: Duration,
    pub avg_latency: Duration,
    pub p50_latency: Duration,
    pub p95_latency: Duration,
    pub p99_latency: Duration,
}

impl PropagationMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_publish(&mut self) {
        self.source_publish_time = Some(Instant::now());
    }

    pub fn record_reception(&mut self, node_id: NodeId) {
        let now = Instant::now();
        self.node_reception_times.insert(node_id, now);

        if let Some(publish_time) = self.source_publish_time {
            let latency = now.duration_since(publish_time);
            self.node_latencies.insert(node_id, latency);
        }
    }

    pub fn calculate_stats(&mut self) {
        if self.node_latencies.is_empty() {
            return;
        }

        let mut latencies: Vec<Duration> = self.node_latencies.values().cloned().collect();
        latencies.sort();

        self.min_latency = *latencies.first().unwrap();
        self.max_latency = *latencies.last().unwrap();

        let total: Duration = latencies.iter().sum();
        self.avg_latency = total / latencies.len() as u32;

        // Percentiles
        let p50_idx = latencies.len() / 2;
        let p95_idx = (latencies.len() as f64 * 0.95) as usize;
        let p99_idx = (latencies.len() as f64 * 0.99) as usize;

        self.p50_latency = latencies.get(p50_idx).cloned().unwrap_or_default();
        self.p95_latency = latencies.get(p95_idx.min(latencies.len() - 1)).cloned().unwrap_or_default();
        self.p99_latency = latencies.get(p99_idx.min(latencies.len() - 1)).cloned().unwrap_or_default();
    }
}

/// État de détection d'un nœud
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Trusted,
    Suspected,
    Exposed,
}

/// Métriques de précision de détection (Metric 5)
#[derive(Debug, Clone, Default)]
pub struct AccuracyMetrics {
    pub total_nodes: usize,
    pub faulty_nodes: Vec<NodeId>,
    pub correct_nodes: Vec<NodeId>,
    pub nodes_exposed: Vec<NodeId>,
    pub nodes_suspected: Vec<NodeId>,
    pub nodes_trusted: Vec<NodeId>,

    // Confusion matrix
    pub true_positives: usize,  // Faulty nodes correctly detected
    pub false_positives: usize, // Correct nodes incorrectly exposed
    pub true_negatives: usize,  // Correct nodes correctly trusted
    pub false_negatives: usize, // Faulty nodes not detected

    // Scores
    pub detection_rate: f64,    // TP / (TP + FN) - Recall
    pub precision: f64,         // TP / (TP + FP)
    pub accuracy: f64,          // (TP + TN) / Total
}

impl AccuracyMetrics {
    pub fn new(total_nodes: usize) -> Self {
        Self {
            total_nodes,
            ..Default::default()
        }
    }

    pub fn set_faulty_nodes(&mut self, nodes: Vec<NodeId>) {
        self.faulty_nodes = nodes;
    }

    pub fn record_status(&mut self, node_id: NodeId, status: NodeStatus) {
        match status {
            NodeStatus::Trusted => self.nodes_trusted.push(node_id),
            NodeStatus::Suspected => self.nodes_suspected.push(node_id),
            NodeStatus::Exposed => self.nodes_exposed.push(node_id),
        }
    }

    pub fn calculate_scores(&mut self) {
        self.correct_nodes = (1..=self.total_nodes as u32)
            .filter(|n| !self.faulty_nodes.contains(n))
            .collect();

        // Calculate confusion matrix
        for &node in &self.nodes_exposed {
            if self.faulty_nodes.contains(&node) {
                self.true_positives += 1;
            } else {
                self.false_positives += 1;
            }
        }

        for &node in &self.nodes_trusted {
            if self.correct_nodes.contains(&node) {
                self.true_negatives += 1;
            } else {
                self.false_negatives += 1;
            }
        }

        // Include suspected as potential positives for detection rate
        for &node in &self.nodes_suspected {
            if self.faulty_nodes.contains(&node) {
                self.true_positives += 1;
            }
        }

        // Calculate scores
        let tp_fn = self.true_positives + self.false_negatives;
        let tp_fp = self.true_positives + self.false_positives;
        let total = self.total_nodes;

        self.detection_rate = if tp_fn > 0 {
            self.true_positives as f64 / tp_fn as f64 * 100.0
        } else {
            100.0 // No faults to detect
        };

        self.precision = if tp_fp > 0 {
            self.true_positives as f64 / tp_fp as f64 * 100.0
        } else {
            100.0 // No positives
        };

        self.accuracy = if total > 0 {
            (self.true_positives + self.true_negatives) as f64 / total as f64 * 100.0
        } else {
            100.0
        };
    }
}

/// Agrégateur de toutes les métriques de test
#[derive(Debug, Clone)]
pub struct TestMetrics {
    pub test_name: String,
    pub content_delivery: ContentDeliveryMetrics,
    pub network_traffic: NetworkTrafficMetrics,
    pub detection_time: DetectionTimeMetrics,
    pub propagation: PropagationMetrics,
    pub accuracy: AccuracyMetrics,
    pub test_start: Instant,
    pub test_duration: Duration,
}

impl TestMetrics {
    pub fn new(test_name: &str, total_nodes: usize) -> Self {
        let mut content_delivery = ContentDeliveryMetrics::new();
        content_delivery.set_total_nodes(total_nodes);

        Self {
            test_name: test_name.to_string(),
            content_delivery,
            network_traffic: NetworkTrafficMetrics::new(),
            detection_time: DetectionTimeMetrics::new(),
            propagation: PropagationMetrics::new(),
            accuracy: AccuracyMetrics::new(total_nodes),
            test_start: Instant::now(),
            test_duration: Duration::ZERO,
        }
    }

    pub fn finalize(&mut self) {
        self.test_duration = self.test_start.elapsed();
        self.content_delivery.calculate_rates();
        self.network_traffic.finalize();
        self.propagation.calculate_stats();
        self.accuracy.calculate_scores();
    }

    /// Sauvegarde les métriques dans un fichier CSV
    pub fn save_to_csv(&self, path: &str) -> std::io::Result<()> {
        let mut file = File::create(path)?;

        writeln!(file, "# Test: {}", self.test_name)?;
        writeln!(file, "# Duration: {:?}", self.test_duration)?;
        writeln!(file)?;

        // Content Delivery
        writeln!(file, "## Content Delivery")?;
        writeln!(file, "total_chunks_sent,{}", self.content_delivery.total_chunks_sent)?;
        writeln!(file, "avg_delivery_rate,{:.2}%", self.content_delivery.avg_delivery_rate)?;
        writeln!(file, "min_delivery_rate,{:.2}%", self.content_delivery.min_delivery_rate)?;
        writeln!(file, "max_delivery_rate,{:.2}%", self.content_delivery.max_delivery_rate)?;
        writeln!(file)?;

        // Network Traffic
        writeln!(file, "## Network Traffic")?;
        writeln!(file, "total_messages_sent,{}", self.network_traffic.total_messages_sent)?;
        writeln!(file, "total_bytes_sent,{}", self.network_traffic.total_bytes_sent)?;
        writeln!(file, "peerreview_overhead_bytes,{}", self.network_traffic.peerreview_overhead_bytes)?;
        writeln!(file, "peerreview_overhead_pct,{:.2}%", self.network_traffic.peerreview_overhead_pct)?;
        writeln!(file, "avg_kbps_per_node,{:.2}", self.network_traffic.avg_kbps_per_node())?;
        writeln!(file)?;

        // Detection Time
        writeln!(file, "## Detection Time")?;
        writeln!(file, "time_to_detection_ms,{}",
            self.detection_time.time_to_detection_ms.unwrap_or(0))?;
        writeln!(file, "detection_method,{:?}", self.detection_time.detection_method)?;
        writeln!(file, "detected_by_count,{}", self.detection_time.detected_by.len())?;
        writeln!(file)?;

        // Propagation Latency
        writeln!(file, "## Propagation Latency")?;
        writeln!(file, "min_latency_ms,{}", self.propagation.min_latency.as_millis())?;
        writeln!(file, "max_latency_ms,{}", self.propagation.max_latency.as_millis())?;
        writeln!(file, "avg_latency_ms,{}", self.propagation.avg_latency.as_millis())?;
        writeln!(file, "p50_latency_ms,{}", self.propagation.p50_latency.as_millis())?;
        writeln!(file, "p95_latency_ms,{}", self.propagation.p95_latency.as_millis())?;
        writeln!(file, "p99_latency_ms,{}", self.propagation.p99_latency.as_millis())?;
        writeln!(file)?;

        // Accuracy
        writeln!(file, "## Fault Detection Accuracy")?;
        writeln!(file, "total_nodes,{}", self.accuracy.total_nodes)?;
        writeln!(file, "faulty_nodes,{}", self.accuracy.faulty_nodes.len())?;
        writeln!(file, "nodes_exposed,{}", self.accuracy.nodes_exposed.len())?;
        writeln!(file, "nodes_suspected,{}", self.accuracy.nodes_suspected.len())?;
        writeln!(file, "nodes_trusted,{}", self.accuracy.nodes_trusted.len())?;
        writeln!(file, "true_positives,{}", self.accuracy.true_positives)?;
        writeln!(file, "false_positives,{}", self.accuracy.false_positives)?;
        writeln!(file, "true_negatives,{}", self.accuracy.true_negatives)?;
        writeln!(file, "false_negatives,{}", self.accuracy.false_negatives)?;
        writeln!(file, "detection_rate,{:.2}%", self.accuracy.detection_rate)?;
        writeln!(file, "precision,{:.2}%", self.accuracy.precision)?;
        writeln!(file, "accuracy,{:.2}%", self.accuracy.accuracy)?;

        Ok(())
    }

    /// Affiche un résumé des métriques
    pub fn print_summary(&self) {
        println!("\n{:=^60}", format!(" {} ", self.test_name));
        println!("Duration: {:?}", self.test_duration);
        println!();

        println!("Content Delivery:");
        println!("  - Chunks sent: {}", self.content_delivery.total_chunks_sent);
        println!("  - Avg delivery rate: {:.2}%", self.content_delivery.avg_delivery_rate);
        println!();

        println!("Network Traffic:");
        println!("  - Total messages: {}", self.network_traffic.total_messages_sent);
        println!("  - Total bytes: {} KB", self.network_traffic.total_bytes_sent / 1024);
        println!("  - PR overhead: {:.2}%", self.network_traffic.peerreview_overhead_pct);
        println!();

        println!("Detection:");
        if let Some(ms) = self.detection_time.time_to_detection_ms {
            println!("  - Time to detection: {} ms", ms);
            println!("  - Method: {:?}", self.detection_time.detection_method);
        } else {
            println!("  - No fault detected (baseline or no fault)");
        }
        println!();

        println!("Propagation Latency:");
        println!("  - Min: {} ms", self.propagation.min_latency.as_millis());
        println!("  - Avg: {} ms", self.propagation.avg_latency.as_millis());
        println!("  - Max: {} ms", self.propagation.max_latency.as_millis());
        println!("  - P95: {} ms", self.propagation.p95_latency.as_millis());
        println!();

        println!("Accuracy:");
        println!("  - Detection rate: {:.2}%", self.accuracy.detection_rate);
        println!("  - Precision: {:.2}%", self.accuracy.precision);
        println!("  - False positives: {}", self.accuracy.false_positives);
        println!("  - False negatives: {}", self.accuracy.false_negatives);
        println!("{:=^60}", "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_delivery_metrics() {
        let mut metrics = ContentDeliveryMetrics::new();
        metrics.set_total_nodes(5); // 5 nœuds dans le test

        for _ in 0..10 {
            metrics.record_send();
        }

        // 5 nœuds reçoivent tous les 10 chunks
        for node in 1..=5 {
            for _ in 0..10 {
                metrics.record_receive(node);
            }
        }

        metrics.calculate_rates();

        assert_eq!(metrics.total_chunks_sent, 10);
        // 5 nœuds sur 5 ont reçu = 100%
        assert_eq!(metrics.avg_delivery_rate, 100.0);
    }

    #[test]
    fn test_accuracy_metrics() {
        let mut metrics = AccuracyMetrics::new(10);

        metrics.set_faulty_nodes(vec![2]); // Node 2 is faulty

        // Node 2 correctly exposed
        metrics.record_status(2, NodeStatus::Exposed);

        // Other nodes correctly trusted
        for node in [1, 3, 4, 5, 6, 7, 8, 9, 10] {
            metrics.record_status(node, NodeStatus::Trusted);
        }

        metrics.calculate_scores();

        assert_eq!(metrics.true_positives, 1);
        assert_eq!(metrics.false_positives, 0);
        assert_eq!(metrics.true_negatives, 9);
        assert_eq!(metrics.false_negatives, 0);
        assert_eq!(metrics.detection_rate, 100.0);
        assert_eq!(metrics.precision, 100.0);
    }
}
