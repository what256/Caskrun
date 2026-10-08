#!/usr/bin/env python3
"""Build a metadata-only compatibility snapshot from a reviewed Highball revision.

No network runs during the build. The source directory must hold db/games/*.json
and db/reports/*.jsonl fetched from SOURCE_REVISION. Only the allowlisted fields
below can enter Caskrun's catalog; contributor identities and narrative text never
enter the output. The parent project downloads sources into its ignored .build/.
"""
import argparse
import datetime
import hashlib
import json
import os
import pathlib
import re
import sys
import tempfile

SOURCE_REVISION = "d8d165834cd0b77613ac0f00300e8fac0b98b143"
REPOSITORY = "https://github.com/gauthierpiarrette/highball-db"
RENDERERS = {"wined3d", "dxmt", "d3dmetal", "dxvk", "vkd3d"}
MAX_FILE_BYTES = 1024 * 1024
MAX_GAMES = 1024
MAX_REPORTS = 4096


def text(value, maximum):
    if not isinstance(value, str) or not value or len(value) > maximum:
        raise ValueError("Invalid bounded text in compatibility source")
    if any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in value):
        raise ValueError("Control character in compatibility source")
    return value


def read(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_FILE_BYTES:
        raise ValueError("Compatibility source must be a bounded regular file")
    return path.read_text(encoding="utf-8")


def app_id(value):
    if isinstance(value, bool):
        return None
    if isinstance(value, int) or (isinstance(value, str) and re.fullmatch(r"[1-9][0-9]{0,9}", value)):
        number = int(value)
        if 0 < number <= 0xFFFFFFFF:
            return number
    return None


def optional_match(value, pattern):
    return value if isinstance(value, str) and re.fullmatch(pattern, value) else None


def recorded_engine(value):
    if not isinstance(value, str):
        return None
    # Two upstream reports wrap the engine ID in a descriptive sentence. Keep
    # only the same public package ID the remaining reports record directly.
    matches = re.findall(r"(?<![A-Za-z0-9])x64-(?:sikarugir|crossover|wine)[A-Za-z0-9._+-]{1,72}", value, re.I)
    return matches[0].lower() if len(matches) == 1 else None


def build(source):
    tree = json.loads(read(source / "tree.json"))
    if tree.get("sha") != SOURCE_REVISION or tree.get("truncated") is not False:
        raise ValueError("Source tree is not the complete reviewed revision")
    expected_blobs = {item["path"]: item["sha"] for item in tree.get("tree", []) if item.get("type") == "blob"}
    expected_paths = {path for path in expected_blobs if
                      (path.startswith("db/games/") and path.endswith(".json")) or
                      (path.startswith("db/reports/") and path.endswith(".jsonl"))}
    actual_paths = {path.relative_to(source).as_posix() for directory, extension in
                    [(source / "db/games", "*.json"), (source / "db/reports", "*.jsonl")]
                    for path in directory.glob(extension)}
    if actual_paths != expected_paths:
        raise ValueError("Compatibility source is missing or adds reviewed files")

    def verified_read(path):
        contents = read(path)
        relative = path.relative_to(source).as_posix()
        raw = contents.encode("utf-8")
        digest = hashlib.sha1(b"blob " + str(len(raw)).encode("ascii") + b"\0" + raw).hexdigest()
        if expected_blobs.get(relative) != digest:
            raise ValueError("Compatibility source differs from the reviewed Git tree")
        return contents

    names = {}
    files = sorted((source / "db/games").glob("*.json"))
    if not files or len(files) > MAX_GAMES:
        raise ValueError("Missing or oversized compatibility game source")
    for path in files:
        game = json.loads(verified_read(path))
        number = app_id(game.get("steam_appid"))
        if number is None:
            continue
        if number in names:
            raise ValueError("Duplicate Steam app ID in compatibility source")
        names[number] = {"steam_app_id": number, "title": text(game.get("title"), 200), "reports": []}

    seen_issues = set()
    report_count = 0
    report_files = sorted((source / "db/reports").glob("*.jsonl"))
    if not report_files or len(report_files) > MAX_REPORTS:
        raise ValueError("Missing or oversized compatibility report source")
    for path in report_files:
        for line_number, line in enumerate(verified_read(path).splitlines(), 1):
            if not line.strip():
                continue
            report_count += 1
            if report_count > MAX_REPORTS:
                raise ValueError("Too many compatibility reports")
            report = json.loads(line)
            number = app_id(report.get("steam_appid"))
            if number is None:
                continue
            if number not in names:
                raise ValueError("Report has no matching Steam game")
            rating = report.get("rating")
            if rating is None:
                continue
            if str(rating) not in {"1", "2", "3", "4", "5"}:
                raise ValueError("Invalid community rating")
            issue = report.get("issue")
            if issue is None:
                if not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*\.jsonl", path.name):
                    raise ValueError("Invalid direct report source filename")
                source_url = REPOSITORY + "/blob/" + SOURCE_REVISION + "/db/reports/" + path.name + "#L" + str(line_number)
            else:
                if type(issue) is not int or not 0 < issue <= 1000000000 or issue in seen_issues:
                    raise ValueError("Invalid or duplicate community issue")
                seen_issues.add(issue)
                source_url = REPOSITORY + "/issues/" + str(issue)
            date = text(report.get("date"), 32)[:10]
            datetime.date.fromisoformat(date)
            renderer = report.get("renderer")
            if renderer is not None and renderer not in RENDERERS:
                raise ValueError("Unknown reported renderer")
            names[number]["reports"].append({
                "rating": int(rating),
                "renderer": renderer,
                "chip": optional_match(report.get("chip"), r"Apple M[1-9][0-9]?(?: Pro| Max| Ultra)?"),
                "macos": optional_match(report.get("macos"), r"[1-9][0-9]?(?:\.[0-9]{1,2}){0,2}"),
                "memory_gib": None,  # Direct reports do not record memory. Never infer it.
                "date": date,
                "engine": recorded_engine(report.get("engine")),
                "source_url": source_url,
            })
    for game in names.values():
        game["reports"].sort(key=lambda report: (report["date"], report["source_url"]), reverse=True)
    return {
        "format": "caskrun-game-compatibility",
        "version": 1,
        "source": {
            "name": "Highball community reports",
            "repository": REPOSITORY,
            "revision": SOURCE_REVISION,
            "license": "CC0-1.0",
            "license_url": REPOSITORY + "/blob/" + SOURCE_REVISION + "/LICENSE",
        },
        "games": sorted(names.values(), key=lambda game: (game["title"].casefold(), game["steam_app_id"])),
    }


def validate_snapshot(value):
    if set(value) != {"format", "version", "source", "games"} or value["format"] != "caskrun-game-compatibility" or value["version"] != 1:
        raise ValueError("Unsupported compatibility snapshot")
    expected = {"name", "repository", "revision", "license", "license_url"}
    source = value["source"]
    if not isinstance(source, dict) or set(source) != expected:
        raise ValueError("Invalid compatibility source")
    if source != {"name": "Highball community reports", "repository": REPOSITORY, "revision": SOURCE_REVISION,
                  "license": "CC0-1.0", "license_url": REPOSITORY + "/blob/" + SOURCE_REVISION + "/LICENSE"}:
        raise ValueError("Unreviewed compatibility source")
    games = value["games"]
    if not isinstance(games, list) or not 0 < len(games) <= MAX_GAMES:
        raise ValueError("Invalid game count")
    ids, sources, count = set(), set(), 0
    fields = {"rating", "renderer", "chip", "macos", "memory_gib", "date", "engine", "source_url"}
    for game in games:
        if set(game) != {"steam_app_id", "title", "reports"} or type(game["steam_app_id"]) is not int or app_id(game["steam_app_id"]) is None or game["steam_app_id"] in ids:
            raise ValueError("Invalid or duplicate compatibility game")
        ids.add(game["steam_app_id"])
        text(game["title"], 200)
        if not isinstance(game["reports"], list):
            raise ValueError("Invalid reports")
        for report in game["reports"]:
            count += 1
            if count > MAX_REPORTS or set(report) != fields:
                raise ValueError("Invalid compatibility report fields")
            if type(report["rating"]) is not int or not 1 <= report["rating"] <= 5:
                raise ValueError("Invalid rating")
            if report["renderer"] is not None and report["renderer"] not in RENDERERS:
                raise ValueError("Invalid renderer")
            for key, pattern in [("chip", r"Apple M[1-9][0-9]?(?: Pro| Max| Ultra)?"), ("macos", r"[1-9][0-9]?(?:\.[0-9]{1,2}){0,2}"),
                                 ("engine", r"x64-(?:sikarugir|crossover|wine)[A-Za-z0-9._+-]{1,72}")]:
                if report[key] is not None and optional_match(report[key], pattern) is None:
                    raise ValueError("Invalid generic report metadata")
            if report["memory_gib"] is not None and (type(report["memory_gib"]) is not int or not 1 <= report["memory_gib"] <= 1024):
                raise ValueError("Invalid reported memory")
            if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", report["date"]):
                raise ValueError("Invalid report date")
            datetime.date.fromisoformat(report["date"])
            url = report["source_url"]
            source_pattern = re.escape(REPOSITORY) + r"/(?:issues/[1-9][0-9]{0,9}|blob/" + SOURCE_REVISION + r"/db/reports/[a-z0-9]+(?:-[a-z0-9]+)*\.jsonl#L[1-9][0-9]{0,5})"
            if not re.fullmatch(source_pattern, url) or url in sources:
                raise ValueError("Invalid or duplicate report source URL")
            sources.add(url)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parents[1] / "catalog/game-compatibility.json")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        value = json.loads(read(args.output))
        validate_snapshot(value)
    else:
        if not args.source_dir:
            parser.error("--source-dir is required when generating")
        value = build(args.source_dir)
        validate_snapshot(value)
        output = json.dumps(value, ensure_ascii=False, indent=2) + "\n"
        if len(output.encode("utf-8")) > MAX_FILE_BYTES:
            raise ValueError("Compatibility snapshot exceeds its size limit")
        if args.output.is_symlink():
            raise ValueError("Refusing a symlink output")
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", prefix=".compatibility-", dir=args.output.parent, delete=False) as file:
                temporary = pathlib.Path(file.name)
                file.write(output)
                file.flush()
                os.fsync(file.fileno())
            os.replace(temporary, args.output)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
    count = sum(len(game["reports"]) for game in value["games"])
    print("Compatibility snapshot validated: {} games, {} external reports".format(len(value["games"]), count))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, TypeError, KeyError) as error:
        print("Compatibility snapshot refused: " + str(error), file=sys.stderr)
        sys.exit(1)
