"""TESSERA Mission 1 / Task A: deterministic synthetic tender ledger forge. Seed-pinned, stdlib-only."""
import csv
import hashlib
import json
import math
import os
import random
from datetime import date, timedelta

SEED = 20260915
TOTAL_ROWS = 5000
OUT_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "evidence", "synthetic")
CSV_PATH = os.path.join(OUT_DIR, "synthetic_tenders.csv")
MANIFEST_PATH = os.path.join(OUT_DIR, "injection_manifest.json")
HASH_PATH = os.path.join(OUT_DIR, "synthetic_tenders.sha256")

DATE_START = date(2024, 1, 1)
DATE_END = date(2026, 9, 10)
DATE_SPAN = (DATE_END - DATE_START).days
AMOUNT_MIN = 2500.0
AMOUNT_MAX = 950000.0
FIELDS = ["transaction_id", "company_name", "owner_name", "amount_usd", "date", "district"]
DISTRICTS = [f"District {i:02d}" for i in range(1, 13)]

PREFIXES = [
    "Aldergate", "Amberley", "Ashford", "Belmont", "Blackwood", "Brookfield", "Cavendish",
    "Clarence", "Copperfield", "Darlington", "Devereux", "Dunmore", "Eastvale", "Elmhurst",
    "Everly", "Fairmont", "Fenwick", "Foxglove", "Glenmore", "Grantham", "Harlow", "Holloway",
    "Inglewood", "Jessup", "Kingsmere", "Kirkwood", "Langford", "Larkspur", "Marlowe",
    "Moorfield", "Norwood", "Northgate", "Oakridge", "Orchard", "Pemberton", "Pinewood",
    "Queensbury", "Radcliffe", "Redstone", "Sandalwood", "Stonebridge", "Thornbury", "Trevor",
    "Underhill", "Ullswater", "Vernon", "Wexford", "Westbrook", "Yorkfield", "Zephyr",
]
CORES = [
    "Apex", "Atlas", "Civic", "Coastal", "Commercial", "Crestline", "Civil", "Harbor",
    "Highland", "Industrial", "Lakeside", "Metro", "Midland", "Pioneer", "Prime", "Reliance",
    "Sovereign", "Summit", "Titan", "Zenith",
]
INDUSTRIES = [
    "Contractors", "Consulting", "Developments", "Engineering", "Enterprises", "Group",
    "Holdings", "Infrastructure", "Logistics", "Partners", "Services", "Solutions", "Trading",
    "Ventures", "Works",
]
SUFFIXES = ["Ltd", "LLC", "Inc", "Co", "PLC"]
FIRST_NAMES = [
    "Aaron", "Adrian", "Alena", "Amos", "Anika", "Antoine", "Beatriz", "Boris", "Branislav",
    "Camille", "Casper", "Cecelia", "Conrad", "Dalia", "Damien", "Delphine", "Dmitri", "Dorian",
    "Elena", "Elias", "Emeline", "Enzo", "Erin", "Estelle", "Evander", "Fabian", "Farida",
    "Felix", "Fiona", "Florian", "Gabriel", "Greta", "Guillaume", "Halvard", "Hanna", "Hector",
    "Helena", "Hugo", "Ilse", "Ivo", "Jacqueline", "Janek", "Jelena", "Jonas", "Julien",
    "Karina", "Kaspar", "Katarina", "Klaus", "Lars", "Laura", "Laurent", "Lena", "Leona",
    "Lucian", "Lucia", "Magda", "Margaux", "Matteo", "Miranda", "Miroslav", "Nadia", "Nikolai",
    "Noor", "Olaf", "Olivia", "Oskar", "Pablo", "Paolina", "Pavel", "Petra", "Quentin",
    "Rafaella", "Rasmus", "Regina", "Rene", "Rosalind", "Rurik", "Sabine", "Samir", "Sanne",
    "Severin", "Silvia", "Stefan", "Tadeusz", "Tatiana", "Thibault", "Ulrike", "Valentin",
    "Vera", "Viktor", "Wilhelm", "Xenia", "Yannick", "Yelena", "Yusuf", "Zara", "Zoran",
]
LAST_NAMES = [
    "Aaltonen", "Andersson", "Arnaud", "Bakker", "Bauer", "Beaumont", "Bergstrom", "Blanchard",
    "Bondarenko", "Braun", "Brogaard", "Capel", "Caruso", "Castellan", "Chaudhry", "Christensen",
    "Clemencic", "Corvin", "Dagher", "Dahlberg", "Delacroix", "Demirci", "Dietrich", "Dobrev",
    "Dupont", "Eekhoff", "Engstrom", "Escobar", "Falkenstein", "Ferreira", "Fischer", "Fontaine",
    "Garrido", "Gauthier", "Georgiev", "Girard", "Graziani", "Halvorsen", "Hartmann", "Haugen",
    "Hoffmann", "Horvath", "Ignatiev", "Ionescu", "Iversen", "Jankowski", "Johannsen",
    "Jovanovic", "Kaminski", "Karlsen", "Kaufmann", "Keller", "Kirkpatrick", "Klassen", "Kovacs",
    "Laine", "Lambert", "Lindqvist", "Lorenz", "Lukacs", "Magnusson", "Marchetti", "Marino",
    "Meier", "Mertens", "Mikkelsen", "Molinari", "Moreau", "Mueller", "Nordin", "Novak",
    "Nyberg", "Oberlin", "Olszynski", "Pedersen", "Pelletier", "Persson", "Petrov", "Pichler",
    "Popescu", "Quaresma", "Radovanovic", "Rasmussen", "Reyes", "Richter", "Rosqvist", "Roth",
    "Saavedra", "Salvati", "Sandberg", "Sauer", "Scheffer", "Schneider", "Sebesta", "Silvestri",
    "Skovgaard", "Slavik", "Stark", "Steffensen", "Szabo", "Tarnowski", "Teodorescu", "Thiel",
    "Thomassen", "Tolvanen", "Trandafir", "Ursu", "Vanek", "Vasquez", "Villanueva", "Vogel",
    "Volkan", "Wagner", "Wallin", "Weber", "Weiss", "Wendel", "Wojcik", "Yilmaz", "Zajac",
    "Zanetti", "Zielinski",
]

RINGS = [
    {
        "ring_id": "RING-01",
        "companies": ["Meridian Axis Ltd", "Basalt Logistics LLC", "Cobalt Ridge Systems Inc"],
        "week_monday": date(2025, 3, 10),
        "base_amount": 487320.00,
        "district": "District 04",
    },
    {
        "ring_id": "RING-02",
        "companies": ["Halcyon Drilling Co", "Verdant Gate Holdings", "Solace Marine Works Ltd"],
        "week_monday": date(2025, 7, 14),
        "base_amount": 262845.00,
        "district": "District 08",
    },
    {
        "ring_id": "RING-03",
        "companies": ["Ironvale Construction Group", "Peregrine Fuel Trading LLC", "Northmoor Equipment Rental"],
        "week_monday": date(2025, 11, 3),
        "base_amount": 731500.00,
        "district": "District 06",
    },
]

BENFORD_CLUSTERS = [
    {"cluster_id": "BENFORD-C1", "district": "District 07", "year": 2025, "month": 8, "rows": 60},
    {"cluster_id": "BENFORD-C2", "district": "District 03", "year": 2025, "month": 12, "rows": 60},
]

ALIAS_PAIRS = [
    {"alias_id": "ALIAS-01", "variants": ["Tongi Builders Ltd", "Tongi Bldrs Limited"],
     "owner": "Rafiqul Hasan", "district": "District 05", "rows_per_variant": 6},
    {"alias_id": "ALIAS-02", "variants": ["Vanguard Steel Works Inc", "Vanguard Steelworks"],
     "owner": "Marek Duvall", "district": "District 02", "rows_per_variant": 6},
    {"alias_id": "ALIAS-03", "variants": ["Bluecrest Marine Services LLC", "Blue Crest Marine Serv."],
     "owner": "Ingrid Sollas", "district": "District 09", "rows_per_variant": 6},
    {"alias_id": "ALIAS-04", "variants": ["Golden Harvest Agro Co.", "Golden Harvest Agricultural Company"],
     "owner": "Tomas Ekstrom", "district": "District 11", "rows_per_variant": 6},
]

rng = random.Random(SEED)
used_companies = set()
used_owners = set()
for ring in RINGS:
    used_companies.update(ring["companies"])
for pair in ALIAS_PAIRS:
    used_companies.update(pair["variants"])
    used_owners.add(pair["owner"])


def benford_amount():
    return round(10 ** rng.uniform(math.log10(AMOUNT_MIN), math.log10(AMOUNT_MAX)), 2)


def random_date():
    return DATE_START + timedelta(days=rng.randint(0, DATE_SPAN))


def new_company():
    while True:
        name = f"{rng.choice(PREFIXES)} {rng.choice(CORES)} {rng.choice(INDUSTRIES)} {rng.choice(SUFFIXES)}"
        if name not in used_companies:
            used_companies.add(name)
            return name


def new_owner():
    while True:
        name = f"{rng.choice(FIRST_NAMES)} {rng.choice(LAST_NAMES)}"
        if name not in used_owners:
            used_owners.add(name)
            return name


def baseline_row():
    return {
        "company_name": new_company(),
        "owner_name": new_owner(),
        "amount_usd": benford_amount(),
        "date": random_date(),
        "district": rng.choice(DISTRICTS),
        "_inj": None,
    }


def ring_rows(ring):
    rows = []
    amount = ring["base_amount"]
    companies = ring["companies"]
    for hop in range(3):
        if hop > 0:
            amount = round(amount * (1.0 - rng.uniform(0.004, 0.015)), 2)
        rows.append({
            "company_name": companies[hop],
            "owner_name": companies[(hop + 1) % 3],
            "amount_usd": amount,
            "date": ring["week_monday"] + timedelta(days=hop * 2),
            "district": ring["district"],
            "_inj": ring["ring_id"],
        })
    return rows


def cluster_rows(cluster):
    rows = []
    for _ in range(cluster["rows"]):
        rows.append({
            "company_name": new_company(),
            "owner_name": new_owner(),
            "amount_usd": round(rng.uniform(90000.0, 99999.99), 2),
            "date": date(cluster["year"], cluster["month"], rng.randint(1, 31)),
            "district": cluster["district"],
            "_inj": cluster["cluster_id"],
        })
    return rows


def alias_rows(pair):
    rows = []
    for variant in pair["variants"]:
        for _ in range(pair["rows_per_variant"]):
            rows.append({
                "company_name": variant,
                "owner_name": pair["owner"],
                "amount_usd": benford_amount(),
                "date": random_date(),
                "district": pair["district"],
                "_inj": pair["alias_id"],
            })
    return rows


rows = []
for ring in RINGS:
    rows.extend(ring_rows(ring))
for cluster in BENFORD_CLUSTERS:
    rows.extend(cluster_rows(cluster))
for pair in ALIAS_PAIRS:
    rows.extend(alias_rows(pair))
baseline_count = TOTAL_ROWS - len(rows)
rows.extend(baseline_row() for _ in range(baseline_count))
rng.shuffle(rows)
for index, row in enumerate(rows, start=1):
    row["transaction_id"] = f"TXN-{index:06d}"

os.makedirs(OUT_DIR, exist_ok=True)
with open(CSV_PATH, "w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=FIELDS, extrasaction="ignore", lineterminator="\r\n")
    writer.writeheader()
    for row in rows:
        row["amount_usd"] = f"{row['amount_usd']:.2f}"
        row["date"] = row["date"].isoformat()
        writer.writerow(row)

with open(CSV_PATH, "rb") as handle:
    csv_sha256 = hashlib.sha256(handle.read()).hexdigest()
with open(HASH_PATH, "w", encoding="utf-8") as handle:
    handle.write(f"{csv_sha256}  synthetic_tenders.csv\n")

injected_rows = [
    {key: row[key] for key in FIELDS + ["_inj"]}
    for row in rows if row["_inj"] is not None
]
manifest = {
    "mission": "TESSERA Mission 1 / Task A",
    "generator": "scripts/forge_synthetic_tenders.py",
    "seed": SEED,
    "total_rows": TOTAL_ROWS,
    "baseline_rows": baseline_count,
    "csv_sha256": csv_sha256,
    "schema_note": (
        "In ring rows (RING-*), owner_name carries the payee entity so the cycle "
        "company_name -> owner_name is expressible in the fixed 6-column schema. Loader rule: if "
        "owner_name matches a known company_name, emit TRANSFERRED_TO edge; else emit OWNED_BY."
    ),
    "injections": {
        "circular_flow_rings": [
            {"ring_id": r["ring_id"], "companies": r["companies"], "week_monday": r["week_monday"].isoformat(),
             "base_amount": r["base_amount"], "district": r["district"]} for r in RINGS
        ],
        "benford_clusters": BENFORD_CLUSTERS,
        "alias_pairs": [
            {"alias_id": a["alias_id"], "variants": a["variants"], "owner": a["owner"], "district": a["district"]}
            for a in ALIAS_PAIRS
        ],
    },
    "injected_rows": injected_rows,
}
with open(MANIFEST_PATH, "w", encoding="utf-8") as handle:
    json.dump(manifest, handle, indent=2)

print(f"rows_written={len(rows)} baseline={baseline_count} injected={len(injected_rows)}")
print(f"csv_sha256={csv_sha256}")
print(f"csv_path={CSV_PATH}")
