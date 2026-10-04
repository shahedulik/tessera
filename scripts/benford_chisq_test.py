"""TESSERA Mission 1 / Task C: Benford first-digit chi-square gate. Stdlib math with scipy cross-verification."""
import csv
import json
import math
import os
import sys

BASE_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "evidence", "synthetic")
CSV_PATH = sys.argv[1] if len(sys.argv) > 1 else os.path.join(BASE_DIR, "synthetic_tenders.csv")
MANIFEST_PATH = os.path.join(BASE_DIR, "injection_manifest.json")
ALPHA = 0.05
DF = 8
BENFORD_P = {d: math.log10(1.0 + 1.0 / d) for d in range(1, 10)}


def _gser(a, x):
    ap = a
    total = 1.0 / a
    delta = total
    for _ in range(10000):
        ap += 1.0
        delta *= x / ap
        total += delta
        if abs(delta) < abs(total) * 1e-16:
            break
    return total * math.exp(-x + a * math.log(x) - math.lgamma(a))


def _gcf(a, x):
    tiny = 1e-300
    b = x + 1.0 - a
    c = 1.0 / tiny
    d = 1.0 / b
    h = d
    for i in range(1, 10000):
        an = -i * (i - a)
        b += 2.0
        d = an * d + b
        if abs(d) < tiny:
            d = tiny
        c = b + an / c
        if abs(c) < tiny:
            c = tiny
        d = 1.0 / d
        delta = d * c
        h *= delta
        if abs(delta - 1.0) < 1e-16:
            break
    return math.exp(-x + a * math.log(x) - math.lgamma(a)) * h


def chi2_sf(x, df):
    a = df / 2.0
    xx = x / 2.0
    if xx < a + 1.0:
        return 1.0 - _gser(a, xx)
    return _gcf(a, xx)


def fmt_p(p):
    return "< 1e-300" if p == 0.0 else f"{p:.6e}"


def run_test(amounts, label):
    n = len(amounts)
    observed = {d: 0 for d in range(1, 10)}
    for amount in amounts:
        observed[int(str(int(amount))[0])] += 1
    chi2 = 0.0
    table = []
    for d in range(1, 10):
        expected = n * BENFORD_P[d]
        chi2 += (observed[d] - expected) ** 2 / expected
        table.append((d, observed[d], expected))
    p = chi2_sf(chi2, DF)
    print(f"\n[{label}]  n = {n}")
    print(f"{'digit':>5} {'observed':>9} {'expected':>10} {'deviation':>10}")
    for d, o, e in table:
        print(f"{d:>5} {o:>9} {e:>10.1f} {o - e:>+10.1f}")
    print(f"chi2 = {chi2:.3f}   df = {DF}   p-value = {fmt_p(p)}")
    return chi2, p


calibration = chi2_sf(15.5073, DF)
assert abs(calibration - 0.05) < 1e-3, f"chi2_sf calibration failed: {calibration}"
print("TESSERA // TASK C // BENFORD FIRST-DIGIT CHI-SQUARE TEST")
print(f"file = {CSV_PATH}")
print(f"calibration: chi2_sf(15.5073, df=8) = {calibration:.4f}  (critical value at alpha=0.05: OK)")

with open(CSV_PATH, newline="", encoding="utf-8") as handle:
    rows = list(csv.DictReader(handle))
amounts = [float(r["amount_usd"]) for r in rows]

chi2_all, p_all = run_test(amounts, "FULL FILE")

try:
    from scipy import stats
    observed = [0] * 9
    for a in amounts:
        observed[int(str(int(a))[0]) - 1] += 1
    expected = [len(amounts) * BENFORD_P[d] for d in range(1, 10)]
    scipy_chi2, scipy_p = stats.chisquare(observed, f_exp=expected)
    print(f"\nscipy cross-verification: chi2 = {scipy_chi2:.3f}   p-value = {scipy_p:.6e}")
    assert abs(scipy_chi2 - chi2_all) < 1e-6 and abs(scipy_p - p_all) / max(scipy_p, 1e-300) < 1e-6
    print("scipy vs stdlib implementation: MATCH")
except ImportError:
    print("\nscipy not available: stdlib implementation stands alone (calibrated against df=8 critical value)")

with open(MANIFEST_PATH, encoding="utf-8") as handle:
    manifest = json.load(handle)
by_id = {r["transaction_id"]: r for r in rows}
for cluster_id in ("BENFORD-C1", "BENFORD-C2"):
    cluster_amounts = [
        float(by_id[e["transaction_id"]]["amount_usd"])
        for e in manifest["injected_rows"] if e["_inj"] == cluster_id
    ]
    _, p_cluster = run_test(cluster_amounts, f"INJECTED CLUSTER {cluster_id} (manifest-scoped)")
    assert p_cluster < ALPHA

print("\n" + "=" * 64)
verdict = "DETECTABLE" if p_all < ALPHA else "NOT DETECTABLE"
print(f"VERDICT: full-file p-value = {fmt_p(p_all)}  ->  p < {ALPHA}  ->  {verdict}")
print(f"Injected digit-9 clusters are statistically detectable by the Benford gate (Manual v1.0 s5.2).")
print("=" * 64)
sys.exit(0 if p_all < ALPHA else 1)
