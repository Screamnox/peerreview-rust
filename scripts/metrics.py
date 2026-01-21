#!/usr/bin/env python3
"""
PeerReview Metrics Dashboard - 5 Metriques Obligatoires
========================================================

Les 5 metriques obligatoires:
1. Content Delivery Rate - % de chunks recus par noeud
2. Network Traffic - Bytes/messages envoyes par noeud
3. Detection Time - Temps entre injection faute -> detection
4. Propagation Latency - Temps source -> dernier noeud
5. Fault Detection Accuracy - Precision (FP, FN, detection rate)

Usage:
    python3 scripts/metrics.py --run-tests    # Run Rust tests and show results
    python3 scripts/metrics.py --live         # Monitor running Docker nodes
    python3 scripts/metrics.py --all          # Run both
"""

import subprocess
import sys
import os
import re
import json
import time
import argparse
from datetime import datetime
from collections import defaultdict

try:
    import matplotlib.pyplot as plt
    import matplotlib.patches as mpatches
    import numpy as np
    HAS_MATPLOTLIB = True
except ImportError:
    HAS_MATPLOTLIB = False
    print("Warning: matplotlib not installed. Install with: pip install matplotlib numpy")

try:
    import requests
    HAS_REQUESTS = True
except ImportError:
    HAS_REQUESTS = False


# ============================================================================
# CONFIGURATION
# ============================================================================

PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PEERREVIEW_CRATE = os.path.join(PROJECT_ROOT, "peerreview_protocol")

NODES = {
    'node1': {'port': 8081, 'witness': True},
    'node2': {'port': 8082, 'witness': True},
    'node3': {'port': 8083, 'witness': True},
    'node4': {'port': 8084, 'witness': False},
    'node5': {'port': 8085, 'witness': False},
    'node6': {'port': 8086, 'witness': False},
    'node7': {'port': 8087, 'witness': False},
    'node8': {'port': 8088, 'witness': False},
    'node9': {'port': 8089, 'witness': False},
    'node10': {'port': 8090, 'witness': False},
}


# ============================================================================
# TEST RUNNER - Runs the actual PeerReview tests
# ============================================================================

class TestRunner:
    """Runs the Rust integration tests and parses results."""

    def __init__(self):
        self.results = {
            'baseline': {},
            'tampering': {},
            'silent': {},
            'fork': {},
        }
        self.metrics = {
            # Metric 1: Content Delivery Rate
            'content_delivery': {
                'chunks_sent': 0,
                'avg_delivery_rate': 0.0,
                'min_delivery_rate': 0.0,
                'max_delivery_rate': 0.0,
                'per_node': {},
            },
            # Metric 2: Network Traffic
            'network_traffic': {
                'total_messages': 0,
                'total_bytes': 0,
                'pr_overhead_pct': 0.0,
                'per_node': {},
            },
            # Metric 3: Detection Time
            'detection_time': {
                'time_to_detection_ms': 0,
                'detection_method': 'None',
                'detected_by_count': 0,
            },
            # Metric 4: Propagation Latency
            'propagation_latency': {
                'min_ms': 0,
                'avg_ms': 0,
                'max_ms': 0,
                'p50_ms': 0,
                'p95_ms': 0,
            },
            # Metric 5: Fault Detection Accuracy
            'accuracy': {
                'true_positives': 0,
                'false_positives': 0,
                'true_negatives': 0,
                'false_negatives': 0,
                'detection_rate': 0.0,
                'precision': 0.0,
            },
            # Per-test detection rates
            'fault_detection': {
                'tampering': {'rate': 0.0},
                'silent': {'rate': 0.0},
                'fork': {'rate': 0.0},
            },
        }

    def run_all_tests(self):
        """Run all PeerReview integration tests."""
        print("\n" + "="*70)
        print("  PEERREVIEW TEST SUITE - 5 Metriques Obligatoires")
        print("="*70)

        tests = [
            ('test_1_baseline', 'Baseline - No Faults'),
            ('test_2_tampering', 'Tampering Detection'),
            ('test_3_silent', 'Silent Node Detection'),
            ('test_4_fork', 'Fork/Equivocation Detection'),
        ]

        for test_file, description in tests:
            print(f"\n{'='*70}")
            print(f"  Running: {description}")
            print(f"{'='*70}")
            self._run_test(test_file)

        return self.metrics

    def _run_test(self, test_name):
        """Run a specific test file and parse output."""
        try:
            result = subprocess.run(
                ['cargo', 'test', '--test', test_name, '--', '--nocapture'],
                cwd=PEERREVIEW_CRATE,
                capture_output=True,
                text=True,
                timeout=120
            )

            output = result.stdout + result.stderr
            self._parse_test_output(test_name, output)

            if result.returncode == 0:
                print(f"  [PASSED] {test_name}")
            else:
                print(f"  [FAILED] {test_name}")

            return result.returncode == 0

        except subprocess.TimeoutExpired:
            print(f"  [TIMEOUT] {test_name}")
            return False
        except FileNotFoundError:
            print(f"  [ERROR] cargo not found. Make sure Rust is installed.")
            return False

    def _parse_test_output(self, test_name, output):
        """Parse test output for metrics."""

        # Metric 1: Content Delivery
        match = re.search(r'Chunks sent:\s*(\d+)', output)
        if match:
            self.metrics['content_delivery']['chunks_sent'] = int(match.group(1))

        match = re.search(r'Avg delivery rate:\s*([\d.]+)%', output)
        if match:
            self.metrics['content_delivery']['avg_delivery_rate'] = float(match.group(1))

        # Metric 2: Network Traffic
        match = re.search(r'Total messages:\s*(\d+)', output)
        if match:
            self.metrics['network_traffic']['total_messages'] = int(match.group(1))

        # Try KB format first
        match = re.search(r'Total bytes:\s*(\d+)\s*KB', output)
        if match:
            self.metrics['network_traffic']['total_bytes'] = int(match.group(1)) * 1024
        else:
            # Try plain bytes format
            match = re.search(r'Total bytes:\s*(\d+)', output)
            if match:
                self.metrics['network_traffic']['total_bytes'] = int(match.group(1))

        match = re.search(r'PR.*overhead.*?:\s*([\d.]+)%', output, re.IGNORECASE)
        if match:
            self.metrics['network_traffic']['pr_overhead_pct'] = float(match.group(1))

        # Metric 3: Detection Time
        match = re.search(r'Time to detection:\s*(\d+)\s*ms', output)
        if match:
            self.metrics['detection_time']['time_to_detection_ms'] = int(match.group(1))

        match = re.search(r'Method:\s*(\w+)', output)
        if match:
            self.metrics['detection_time']['detection_method'] = match.group(1)

        # Metric 4: Propagation Latency
        match = re.search(r'Min:\s*(\d+)\s*ms', output)
        if match:
            self.metrics['propagation_latency']['min_ms'] = int(match.group(1))

        match = re.search(r'Avg:\s*(\d+)\s*ms', output)
        if match:
            self.metrics['propagation_latency']['avg_ms'] = int(match.group(1))

        match = re.search(r'Max:\s*(\d+)\s*ms', output)
        if match:
            self.metrics['propagation_latency']['max_ms'] = int(match.group(1))

        match = re.search(r'P95:\s*(\d+)\s*ms', output)
        if match:
            self.metrics['propagation_latency']['p95_ms'] = int(match.group(1))

        # Metric 5: Accuracy
        match = re.search(r'Detection rate:\s*([\d.]+)%', output)
        if match:
            rate = float(match.group(1))
            self.metrics['accuracy']['detection_rate'] = rate

            # Also update per-fault detection
            if 'tampering' in test_name:
                self.metrics['fault_detection']['tampering']['rate'] = rate
            elif 'silent' in test_name:
                self.metrics['fault_detection']['silent']['rate'] = rate
            elif 'fork' in test_name:
                self.metrics['fault_detection']['fork']['rate'] = rate

        match = re.search(r'Precision:\s*([\d.]+)%', output)
        if match:
            self.metrics['accuracy']['precision'] = float(match.group(1))

        match = re.search(r'False positives:\s*(\d+)', output)
        if match:
            self.metrics['accuracy']['false_positives'] = int(match.group(1))

        match = re.search(r'False negatives:\s*(\d+)', output)
        if match:
            self.metrics['accuracy']['false_negatives'] = int(match.group(1))

        # TP and TN from confusion matrix style output
        for metric in ['true_positives', 'false_positives', 'true_negatives', 'false_negatives']:
            pattern = metric.replace('_', ' ').title().replace(' ', ' ')
            match = re.search(rf'{pattern}:\s*(\d+)', output, re.IGNORECASE)
            if match:
                self.metrics['accuracy'][metric] = int(match.group(1))


# ============================================================================
# LIVE METRICS - Monitor running Docker nodes
# ============================================================================

class LiveMetrics:
    """Collect metrics from running Docker nodes."""

    def __init__(self):
        self.stats = {}
        self.prev_stats = {}
        self.metrics = {}

    def fetch_all_stats(self):
        """Fetch stats from all running nodes."""
        if not HAS_REQUESTS:
            print("Error: requests library not installed")
            return {}

        stats = {}
        for node, config in NODES.items():
            try:
                response = requests.get(
                    f"http://localhost:{config['port']}/stats",
                    timeout=2
                )
                if response.status_code == 200:
                    stats[node] = response.json()
            except:
                pass

        self.prev_stats = self.stats
        self.stats = stats
        return stats

    def calculate_metrics(self, interval=1.0):
        """Calculate metrics from live data."""
        if not self.stats:
            return {}

        metrics = {
            'timestamp': datetime.now().isoformat(),
            'nodes_online': len(self.stats),
            'nodes_total': len(NODES),
        }

        # Content Delivery
        total_recv = sum(s.get('recv_total', 0) for s in self.stats.values())
        metrics['total_messages'] = total_recv

        # Network Traffic
        total_pr_sent = sum(s.get('pr_commit_sent', 0) for s in self.stats.values())
        total_pr_recv = sum(s.get('pr_commit_recv', 0) for s in self.stats.values())

        metrics['pr_commits_sent'] = total_pr_sent
        metrics['pr_commits_recv'] = total_pr_recv

        # Per node stats
        metrics['per_node'] = {}
        for node, data in self.stats.items():
            metrics['per_node'][node] = {
                'recv_total': data.get('recv_total', 0),
                'pr_sent': data.get('pr_commit_sent', 0),
                'pr_recv': data.get('pr_commit_recv', 0),
            }

        self.metrics = metrics
        return metrics


# ============================================================================
# VISUALIZATION - 5 Metriques Obligatoires (Images Separees)
# ============================================================================

def create_metric_1_content_delivery(test_metrics, output_dir='metrics_images'):
    """Create separate image for Metric 1: Content Delivery Rate."""
    if not HAS_MATPLOTLIB:
        return None

    os.makedirs(output_dir, exist_ok=True)

    fig, ax = plt.subplots(figsize=(10, 6))
    fig.patch.set_facecolor('white')
    ax.set_facecolor('white')

    ax.set_title('Metrique 1: Content Delivery Rate', fontsize=16, fontweight='bold', color='#333', pad=20)

    cd = test_metrics['content_delivery']

    # Bar chart
    categories = ['Taux de Livraison (%)', 'Chunks Envoyes']
    values = [cd['avg_delivery_rate'], cd['chunks_sent']]
    colors = ['#3498db', '#2ecc71']

    bars = ax.bar(categories, values, color=colors, edgecolor='#333', linewidth=1.5, width=0.5)

    ax.set_ylabel('Valeur', fontsize=12, color='#333')
    ax.tick_params(colors='#333')

    for bar, val in zip(bars, values):
        label = f'{val:.1f}%' if 'Taux' in categories[list(bars).index(bar)] else f'{int(val)}'
        ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + max(values)*0.02,
                label, ha='center', fontsize=14, color='#333', fontweight='bold')

    ax.spines['top'].set_visible(False)
    ax.spines['right'].set_visible(False)
    ax.spines['bottom'].set_color('#333')
    ax.spines['left'].set_color('#333')

    # Add grid
    ax.yaxis.grid(True, linestyle='--', alpha=0.3)
    ax.set_axisbelow(True)

    plt.tight_layout()
    filepath = os.path.join(output_dir, 'metrique_1_content_delivery.png')
    fig.savefig(filepath, dpi=150, bbox_inches='tight', facecolor='white', edgecolor='none')
    plt.close(fig)
    return filepath


def create_metric_2_network_traffic(test_metrics, output_dir='metrics_images'):
    """Create separate image for Metric 2: Network Traffic."""
    if not HAS_MATPLOTLIB:
        return None

    os.makedirs(output_dir, exist_ok=True)

    fig, ax = plt.subplots(figsize=(10, 6))
    fig.patch.set_facecolor('white')
    ax.set_facecolor('white')

    ax.set_title('Metrique 2: Network Traffic', fontsize=16, fontweight='bold', color='#333', pad=20)

    nt = test_metrics['network_traffic']

    categories = ['Total Messages', 'Total Bytes (KB)', 'PR Overhead (%)']
    values = [
        nt['total_messages'],
        nt['total_bytes'] / 1024 if nt['total_bytes'] >= 1024 else nt['total_bytes'],
        nt['pr_overhead_pct']
    ]
    colors = ['#3498db', '#f39c12', '#e74c3c']

    bars = ax.bar(categories, values, color=colors, edgecolor='#333', linewidth=1.5, width=0.5)

    ax.set_ylabel('Valeur', fontsize=12, color='#333')
    ax.tick_params(colors='#333')

    for bar, val, cat in zip(bars, values, categories):
        label = f'{val:.1f}%' if 'Overhead' in cat else f'{int(val)}'
        ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + max(values)*0.02,
                label, ha='center', fontsize=12, color='#333', fontweight='bold')

    ax.spines['top'].set_visible(False)
    ax.spines['right'].set_visible(False)
    ax.spines['bottom'].set_color('#333')
    ax.spines['left'].set_color('#333')

    ax.yaxis.grid(True, linestyle='--', alpha=0.3)
    ax.set_axisbelow(True)

    plt.tight_layout()
    filepath = os.path.join(output_dir, 'metrique_2_network_traffic.png')
    fig.savefig(filepath, dpi=150, bbox_inches='tight', facecolor='white', edgecolor='none')
    plt.close(fig)
    return filepath


def create_metric_3_detection_time(test_metrics, output_dir='metrics_images'):
    """Create separate image for Metric 3: Detection Time."""
    if not HAS_MATPLOTLIB:
        return None

    os.makedirs(output_dir, exist_ok=True)

    fig, ax = plt.subplots(figsize=(10, 6))
    fig.patch.set_facecolor('white')
    ax.set_facecolor('white')

    ax.set_title('Metrique 3: Detection Time', fontsize=16, fontweight='bold', color='#333', pad=20)

    dt = test_metrics['detection_time']
    detection_ms = dt['time_to_detection_ms']
    method = dt['detection_method']

    if detection_ms > 0:
        # Gauge visualization
        sizes = [detection_ms, max(100 - detection_ms, 10)]
        colors_pie = ['#e74c3c', '#ecf0f1']
        wedges, _ = ax.pie(sizes, colors=colors_pie, startangle=90,
                           wedgeprops=dict(width=0.4, edgecolor='#333'))

        ax.text(0, 0, f'{detection_ms}\nms', ha='center', va='center',
                fontsize=28, fontweight='bold', color='#333')
        ax.text(0, -0.8, f'Methode: {method}', ha='center', fontsize=12, color='#666')
    else:
        ax.text(0.5, 0.5, 'Pas de Faute Detectee\n(Test Baseline)',
                ha='center', va='center', transform=ax.transAxes,
                fontsize=18, color='#27ae60', fontweight='bold')
        ax.text(0.5, 0.3, '(Comportement attendu pour le test baseline)',
                ha='center', transform=ax.transAxes, fontsize=11, color='#666')

    ax.axis('equal')
    ax.axis('off')

    plt.tight_layout()
    filepath = os.path.join(output_dir, 'metrique_3_detection_time.png')
    fig.savefig(filepath, dpi=150, bbox_inches='tight', facecolor='white', edgecolor='none')
    plt.close(fig)
    return filepath


def create_metric_4_propagation_latency(test_metrics, output_dir='metrics_images'):
    """Create separate image for Metric 4: Propagation Latency."""
    if not HAS_MATPLOTLIB:
        return None

    os.makedirs(output_dir, exist_ok=True)

    fig, ax = plt.subplots(figsize=(10, 6))
    fig.patch.set_facecolor('white')
    ax.set_facecolor('white')

    ax.set_title('Metrique 4: Propagation Latency', fontsize=16, fontweight='bold', color='#333', pad=20)

    pl = test_metrics['propagation_latency']

    labels = ['Min', 'Avg', 'P95', 'Max']
    values = [pl['min_ms'], pl['avg_ms'], pl['p95_ms'], pl['max_ms']]
    colors = ['#2ecc71', '#3498db', '#f39c12', '#e74c3c']

    bars = ax.bar(labels, values, color=colors, edgecolor='#333', linewidth=1.5, width=0.5)

    ax.set_ylabel('Latence (ms)', fontsize=12, color='#333')
    ax.set_xlabel('Percentile', fontsize=12, color='#333')
    ax.tick_params(colors='#333')

    for bar, val in zip(bars, values):
        if val > 0:
            ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + max(values)*0.02,
                    f'{val} ms', ha='center', fontsize=12, color='#333', fontweight='bold')

    ax.spines['top'].set_visible(False)
    ax.spines['right'].set_visible(False)
    ax.spines['bottom'].set_color('#333')
    ax.spines['left'].set_color('#333')

    ax.yaxis.grid(True, linestyle='--', alpha=0.3)
    ax.set_axisbelow(True)

    plt.tight_layout()
    filepath = os.path.join(output_dir, 'metrique_4_propagation_latency.png')
    fig.savefig(filepath, dpi=150, bbox_inches='tight', facecolor='white', edgecolor='none')
    plt.close(fig)
    return filepath


def create_metric_5_accuracy(test_metrics, output_dir='metrics_images'):
    """Create separate image for Metric 5: Fault Detection Accuracy."""
    if not HAS_MATPLOTLIB:
        return None

    os.makedirs(output_dir, exist_ok=True)

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(14, 6))
    fig.patch.set_facecolor('white')

    fig.suptitle('Metrique 5: Fault Detection Accuracy', fontsize=16, fontweight='bold', color='#333', y=1.02)

    acc = test_metrics['accuracy']

    # Left: Confusion Matrix
    ax1.set_facecolor('white')
    ax1.set_title('Matrice de Confusion', fontsize=14, fontweight='bold', color='#333', pad=10)

    tp = acc['true_positives']
    fp = acc['false_positives']
    tn = acc['true_negatives']
    fn = acc['false_negatives']

    matrix = np.array([[tp, fn], [fp, tn]])

    im = ax1.imshow(matrix, cmap='RdYlGn', aspect='auto', vmin=0, vmax=max(tp, tn, fp, fn, 1))

    ax1.set_xticks([0, 1])
    ax1.set_yticks([0, 1])
    ax1.set_xticklabels(['Predit\nFautif', 'Predit\nCorrect'], fontsize=10, color='#333')
    ax1.set_yticklabels(['Reel\nFautif', 'Reel\nCorrect'], fontsize=10, color='#333')

    labels_matrix = [['TP', 'FN'], ['FP', 'TN']]
    for i in range(2):
        for j in range(2):
            val = matrix[i, j]
            color = 'white' if val > matrix.max()/2 else 'black'
            ax1.text(j, i, f'{labels_matrix[i][j]}\n{val}',
                    ha='center', va='center', fontsize=14, fontweight='bold', color=color)

    # Right: Metrics bars
    ax2.set_facecolor('white')
    ax2.set_title('Scores de Performance', fontsize=14, fontweight='bold', color='#333', pad=10)

    metrics_labels = ['Detection Rate', 'Precision']
    metrics_values = [acc['detection_rate'], acc['precision']]
    colors = ['#3498db', '#2ecc71']

    bars = ax2.barh(metrics_labels, metrics_values, color=colors, edgecolor='#333', linewidth=1.5, height=0.5)

    ax2.set_xlim(0, 110)
    ax2.set_xlabel('Pourcentage (%)', fontsize=12, color='#333')
    ax2.axvline(x=100, color='#27ae60', linestyle='--', alpha=0.7, label='Objectif: 100%')

    for bar, val in zip(bars, metrics_values):
        ax2.text(val + 2, bar.get_y() + bar.get_height()/2,
                f'{val:.1f}%', va='center', fontsize=12, color='#333', fontweight='bold')

    ax2.spines['top'].set_visible(False)
    ax2.spines['right'].set_visible(False)
    ax2.spines['bottom'].set_color('#333')
    ax2.spines['left'].set_color('#333')
    ax2.tick_params(colors='#333')
    ax2.legend(loc='lower right', fontsize=10)

    ax2.xaxis.grid(True, linestyle='--', alpha=0.3)
    ax2.set_axisbelow(True)

    plt.tight_layout()
    filepath = os.path.join(output_dir, 'metrique_5_accuracy.png')
    fig.savefig(filepath, dpi=150, bbox_inches='tight', facecolor='white', edgecolor='none')
    plt.close(fig)
    return filepath


def create_all_separate_images(test_metrics, output_dir='metrics_images'):
    """Create all 5 metric images separately."""
    if not HAS_MATPLOTLIB:
        print("Cannot create images: matplotlib not installed")
        return []

    print(f"\nGeneration des images dans le dossier: {output_dir}/")

    files = []

    f1 = create_metric_1_content_delivery(test_metrics, output_dir)
    if f1:
        files.append(f1)
        print(f"  [OK] {f1}")

    f2 = create_metric_2_network_traffic(test_metrics, output_dir)
    if f2:
        files.append(f2)
        print(f"  [OK] {f2}")

    f3 = create_metric_3_detection_time(test_metrics, output_dir)
    if f3:
        files.append(f3)
        print(f"  [OK] {f3}")

    f4 = create_metric_4_propagation_latency(test_metrics, output_dir)
    if f4:
        files.append(f4)
        print(f"  [OK] {f4}")

    f5 = create_metric_5_accuracy(test_metrics, output_dir)
    if f5:
        files.append(f5)
        print(f"  [OK] {f5}")

    print(f"\n5 images generees dans: {output_dir}/")
    return files


def print_metrics_report(test_metrics, live_metrics=None):
    """Print a detailed ASCII report of the 5 metrics."""
    print("\n" + "="*70)
    print("  PEERREVIEW - 5 METRIQUES OBLIGATOIRES")
    print("="*70)

    # 1. Content Delivery Rate
    print("\n" + "-"*70)
    print("  1. CONTENT DELIVERY RATE")
    print("-"*70)
    cd = test_metrics['content_delivery']
    print(f"  Chunks envoyés:        {cd['chunks_sent']}")
    print(f"  Taux de livraison:     {cd['avg_delivery_rate']:.1f}%")

    # 2. Network Traffic
    print("\n" + "-"*70)
    print("  2. NETWORK TRAFFIC")
    print("-"*70)
    nt = test_metrics['network_traffic']
    print(f"  Total messages:        {nt['total_messages']}")
    print(f"  Total bytes:           {nt['total_bytes']} ({nt['total_bytes']/1024:.1f} KB)")
    print(f"  PR Overhead:           {nt['pr_overhead_pct']:.1f}%")

    # 3. Detection Time
    print("\n" + "-"*70)
    print("  3. DETECTION TIME")
    print("-"*70)
    dt = test_metrics['detection_time']
    if dt['time_to_detection_ms'] > 0:
        print(f"  Temps de détection:    {dt['time_to_detection_ms']} ms")
        print(f"  Méthode:               {dt['detection_method']}")
    else:
        print(f"  Pas de faute détectée (baseline)")

    # 4. Propagation Latency
    print("\n" + "-"*70)
    print("  4. PROPAGATION LATENCY")
    print("-"*70)
    pl = test_metrics['propagation_latency']
    print(f"  Min:   {pl['min_ms']} ms")
    print(f"  Avg:   {pl['avg_ms']} ms")
    print(f"  P95:   {pl['p95_ms']} ms")
    print(f"  Max:   {pl['max_ms']} ms")

    # 5. Fault Detection Accuracy
    print("\n" + "-"*70)
    print("  5. FAULT DETECTION ACCURACY")
    print("-"*70)
    acc = test_metrics['accuracy']
    print(f"  True Positives (TP):   {acc['true_positives']}")
    print(f"  False Positives (FP):  {acc['false_positives']}")
    print(f"  True Negatives (TN):   {acc['true_negatives']}")
    print(f"  False Negatives (FN):  {acc['false_negatives']}")
    print()
    print(f"  Detection Rate:        {acc['detection_rate']:.1f}%")
    print(f"  Precision:             {acc['precision']:.1f}%")

    # Detection by fault type
    print("\n" + "-"*70)
    print("  DETECTION PAR TYPE DE FAUTE")
    print("-"*70)
    fd = test_metrics['fault_detection']
    print(f"  Tampering:   {fd['tampering']['rate']:.1f}%")
    print(f"  Silent:      {fd['silent']['rate']:.1f}%")
    print(f"  Fork:        {fd['fork']['rate']:.1f}%")

    print("\n" + "="*70)


def export_csv(test_metrics, filename='metrics_5_obligatoires.csv'):
    """Export the 5 metrics to CSV."""
    import csv

    with open(filename, 'w', newline='') as f:
        writer = csv.writer(f)
        writer.writerow(['Metrique', 'Sous-metrique', 'Valeur', 'Unite'])

        # 1. Content Delivery
        cd = test_metrics['content_delivery']
        writer.writerow(['1. Content Delivery', 'chunks_sent', cd['chunks_sent'], 'count'])
        writer.writerow(['1. Content Delivery', 'avg_delivery_rate', cd['avg_delivery_rate'], '%'])

        # 2. Network Traffic
        nt = test_metrics['network_traffic']
        writer.writerow(['2. Network Traffic', 'total_messages', nt['total_messages'], 'count'])
        writer.writerow(['2. Network Traffic', 'total_bytes', nt['total_bytes'], 'bytes'])
        writer.writerow(['2. Network Traffic', 'pr_overhead', nt['pr_overhead_pct'], '%'])

        # 3. Detection Time
        dt = test_metrics['detection_time']
        writer.writerow(['3. Detection Time', 'time_to_detection', dt['time_to_detection_ms'], 'ms'])
        writer.writerow(['3. Detection Time', 'method', dt['detection_method'], ''])

        # 4. Propagation Latency
        pl = test_metrics['propagation_latency']
        writer.writerow(['4. Propagation Latency', 'min', pl['min_ms'], 'ms'])
        writer.writerow(['4. Propagation Latency', 'avg', pl['avg_ms'], 'ms'])
        writer.writerow(['4. Propagation Latency', 'p95', pl['p95_ms'], 'ms'])
        writer.writerow(['4. Propagation Latency', 'max', pl['max_ms'], 'ms'])

        # 5. Accuracy
        acc = test_metrics['accuracy']
        writer.writerow(['5. Accuracy', 'true_positives', acc['true_positives'], 'count'])
        writer.writerow(['5. Accuracy', 'false_positives', acc['false_positives'], 'count'])
        writer.writerow(['5. Accuracy', 'true_negatives', acc['true_negatives'], 'count'])
        writer.writerow(['5. Accuracy', 'false_negatives', acc['false_negatives'], 'count'])
        writer.writerow(['5. Accuracy', 'detection_rate', acc['detection_rate'], '%'])
        writer.writerow(['5. Accuracy', 'precision', acc['precision'], '%'])

    print(f"\nMetrics exported to: {filename}")


# ============================================================================
# MAIN
# ============================================================================

def main():
    parser = argparse.ArgumentParser(
        description='PeerReview Metrics - 5 Metriques Obligatoires'
    )
    parser.add_argument('--run-tests', action='store_true',
                        help='Run Rust integration tests')
    parser.add_argument('--live', action='store_true',
                        help='Monitor live Docker nodes')
    parser.add_argument('--all', action='store_true',
                        help='Run tests and monitor live nodes')
    parser.add_argument('--no-gui', action='store_true',
                        help='Skip graphical dashboard, only print to terminal')
    parser.add_argument('--export', type=str, default=None,
                        help='Export metrics to CSV file')

    args = parser.parse_args()

    # Default to --run-tests if no args
    if not args.run_tests and not args.live and not args.all:
        args.run_tests = True

    test_metrics = None
    live_metrics = None

    # Run tests
    if args.run_tests or args.all:
        runner = TestRunner()
        test_metrics = runner.run_all_tests()
    else:
        runner = TestRunner()
        test_metrics = runner.metrics

    # Collect live metrics
    if args.live or args.all:
        print("\nCollecting live metrics from Docker nodes...")
        live_collector = LiveMetrics()
        stats = live_collector.fetch_all_stats()

        if stats:
            live_metrics = live_collector.calculate_metrics()
            print(f"Connected to {live_metrics['nodes_online']}/{live_metrics['nodes_total']} nodes")
        else:
            print("No Docker nodes responding. Start with: docker-compose up -d")

    # Print report
    print_metrics_report(test_metrics, live_metrics)

    # Export if requested
    if args.export:
        export_csv(test_metrics, args.export)
    else:
        export_csv(test_metrics)  # Default export

    # Create separate images for each metric
    if not args.no_gui and HAS_MATPLOTLIB:
        create_all_separate_images(test_metrics, output_dir='metrics_images')


if __name__ == "__main__":
    main()
