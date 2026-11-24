"""Saves a GitHub user's public contribution calendar in the shape whisker.rs's API returns.

usage: python3 -I snapshot.py USER... (writes ../assets/snapshots/USER.json)
"""
import json
import os
import re
import sys
import urllib.request

DAY = re.compile(r"<td\b[^>]*\bContributionCalendar-day\b[^>]*>")
TIP = re.compile(r'<tool-tip\b[^>]*\bfor="([^"]+)"[^>]*>([^<]*)</tool-tip>')


def attribute(tag, name):
    found = re.search(rf'\b{name}="([^"]*)"', tag)
    return found and found.group(1)


def calendar(user):
    request = urllib.request.Request(
        f"https://github.com/users/{user}/contributions",
        headers={"User-Agent": "Mozilla/5.0 (commitcity snapshot)"},
    )
    html = urllib.request.urlopen(request, timeout=30).read().decode()
    counts = {}
    for target, text in TIP.findall(html):
        number = re.match(r"([\d,]+|No) contribution", text.strip())
        if number:
            counts[target] = 0 if number.group(1) == "No" else int(number.group(1).replace(",", ""))
    days = []
    for tag in DAY.findall(html):
        day_id, date = attribute(tag, "id"), attribute(tag, "data-date")
        if day_id in counts and date:
            days.append({"date": date, "count": counts[day_id]})
    days.sort(key=lambda d: d["date"])
    assert days, f"no days parsed for {user}"
    return {"user": user, "total": sum(d["count"] for d in days), "days": days}


for user in sys.argv[1:]:
    data = calendar(user)
    path = os.path.join(os.path.dirname(__file__), "..", "assets", "snapshots", f"{user}.json")
    with open(path, "w") as out:
        json.dump(data, out, separators=(",", ":"))
    print(user, len(data["days"]), data["days"][0]["date"], data["days"][-1]["date"], data["total"])
