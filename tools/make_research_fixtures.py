#!/usr/bin/env python3
"""Writes the recorded responses the research tests use.

The game, its wiki, its store page and its news are fictional (they belong to
the "Sky Meadow Online" test game in crates/syrup-testgames), written for
these tests in the formats the real services answer in: the MediaWiki API,
Wikipedia's REST summaries, and Steam's store and news APIs.

    python tools/make_research_fixtures.py
"""
import json
import os
import re

OUT = os.path.join(os.path.dirname(__file__), "..", "crates", "syrup-knowledge", "fixtures")


def fnv64(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def fixture_name(url: str) -> str:
    short = url
    for p in ("https://", "http://"):
        if short.startswith(p):
            short = short[len(p):]
    short = "".join(c.lower() if c.isascii() and c.isalnum() else "_" for c in short)[:80]
    return f"{short}-{fnv64(url.encode()) & 0xFFFFFFFF:08x}.txt"


def put(url, body):
    with open(os.path.join(OUT, fixture_name(url)), "w", encoding="utf-8") as f:
        f.write(body if isinstance(body, str) else json.dumps(body, indent=1))


def enc(s):
    return re.sub(r"[^A-Za-z0-9\-_.~]", lambda m: "%20" if m.group(0) == " " else "%%%02X" % ord(m.group(0)), s)


WIKI = "https://skymeadowonline.fandom.com/api.php"


def wiki_search(query, titles, limit):
    put(f"{WIKI}?action=query&list=search&srsearch={enc(query)}&format=json&srlimit={limit}",
        {"batchcomplete": "", "query": {"searchinfo": {"totalhits": len(titles)}, "search": [{"ns": 0, "title": t, "pageid": i + 1} for i, t in enumerate(titles)]}})


def wiki_page(title, paragraphs):
    html = '<div class="mw-parser-output"><aside class="portable-infobox"><h2>' + title + '</h2><div>Level 30</div></aside>'
    html += "".join(f"<p>{p}<sup class=\"reference\">[{i + 1}]</sup></p>" for i, p in enumerate(paragraphs))
    html += "<table class=\"wikitable\"><tr><th>Drop</th><td>Moss Crown</td></tr></table></div>"
    put(f"{WIKI}?action=parse&page={enc(title)}&prop=text&format=json&formatversion=2&redirects=1",
        {"parse": {"title": title, "pageid": 1, "text": html}})


def main():
    os.makedirs(OUT, exist_ok=True)
    for f in os.listdir(OUT):
        if f.endswith(".txt"):
            os.remove(os.path.join(OUT, f))
    title = "Sky Meadow Online"
    wiki_search(title, [title], 1)
    wiki_search(title, [title], 2)
    wiki_page(title, [
        "<b>Sky Meadow Online</b> is a free-to-play side-scrolling MMORPG set on floating islands. Players level up by defeating monsters and completing quests for the people of Mossy Hills.",
        "Health and mana are shown in the status bar at the bottom of the screen, above the experience bar.",
    ])
    wiki_search("Mossy King", ["Mossy King", "Mossy Hills"], 2)
    wiki_page("Mossy King", [
        "The Mossy King is the boss of Mossy Hills. The Mossy King is weak to fire.",
        "In version 1.2 the Mossy King attacked every 3 seconds.",
        "Players often fight the Mossy King from the left ledge, where its slam cannot reach. It is recommended to keep a potion ready for its second phase.",
        "Some players believe the Mossy King might have a hidden third phase.",
        "The Mossy King drops the Moss Crown.",
        "Defeating the Mossy King reveals that the king is actually the lost gardener.",
    ])
    wiki_page("Mossy Hills", [
        "Mossy Hills is the first region of the game. The Mossy King can be found in Mossy Hills.",
    ])
    put(f"https://store.steampowered.com/api/storesearch/?term={enc(title)}&l=english&cc=US",
        {"total": 1, "items": [{"type": "app", "name": title, "id": 999001, "price": {"currency": "USD", "initial": 0, "final": 0}}]})
    put("https://store.steampowered.com/api/appdetails?appids=999001&l=english",
        {"999001": {"success": True, "data": {"type": "game", "name": title, "steam_appid": 999001,
                                               "short_description": "Explore floating meadows, battle slimes and team up with friends in a cozy side-scrolling MMORPG.",
                                               "genres": [{"id": "29", "description": "Massively Multiplayer"}, {"id": "3", "description": "RPG"}, {"id": "37", "description": "Free to Play"}]}}})
    put("https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid=999001&count=8&maxlength=600&format=json",
        {"appnews": {"appid": 999001, "count": 3, "newsitems": [
            {"gid": "3", "title": "Patch 1.3.1: Mossy King rebalance", "url": "https://store.steampowered.com/news/app/999001/view/3",
             "contents": "The Mossy King now attacks every 4 seconds instead of 3. Potions now restore 45% of maximum HP.", "date": 1789000000},
            {"gid": "2", "title": "Community spotlight: meadow art", "url": "https://store.steampowered.com/news/app/999001/view/2",
             "contents": "Look at these paintings of the meadow.", "date": 1788000000},
            {"gid": "1", "title": "Update 1.3 is live", "url": "https://store.steampowered.com/news/app/999001/view/1",
             "contents": "The Gem Hunter quest was added. The Mossy King drops the Moss Crown more often.", "date": 1786000000},
        ]}})
    put(f"https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch={enc(title + ' video game')}&format=json&srlimit=3",
        {"query": {"search": [{"ns": 0, "title": title, "pageid": 5}]}})
    put("https://en.wikipedia.org/api/rest_v1/page/summary/Sky_Meadow_Online",
        {"type": "standard", "title": title, "description": "2024 massively multiplayer online role-playing game",
         "extract": "Sky Meadow Online is a 2024 side-scrolling massively multiplayer online role-playing game. Reviewers praised its relaxed pace and criticised its difficulty spikes.",
         "content_urls": {"desktop": {"page": "https://en.wikipedia.org/wiki/Sky_Meadow_Online"}}})
    with open(os.path.join(OUT, "README.md"), "w", encoding="utf-8") as f:
        f.write("Recorded responses for the research tests, written by `tools/make_research_fixtures.py`.\n"
                "The game and every text here are fictional: made up for these tests.\n")
    print("fixtures in", os.path.normpath(OUT))


main()
