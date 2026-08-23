import re

with open("script.html") as f:
    text = f.read()

# Let's remove the rogue orphaned blocks before <div class="slug"> in Beat 03 and Beat 05
# Pattern in Beat 03:
text = re.sub(
    r'<!-- 03 -->\s*<section class="beat">\s*<span class="h">🔍 คำอธิบายองค์ประกอบหน้าจอ.*?</div>\s*</div>\s*(<div class="slug">)',
    r'<!-- 03 -->\n<section class="beat">\n  \1',
    text,
    flags=re.DOTALL
)

# Pattern in Beat 05:
text = re.sub(
    r'<!-- 05 -->\s*<section class="beat">\s*<span class="h">🔍 คำอธิบายหน้าต่าง Live Validator CLI.*?</div>\s*</div>\s*(<div class="slug">)',
    r'<!-- 05 -->\n<section class="beat">\n  \1',
    text,
    flags=re.DOTALL
)

# Let's also check if there are any other rogue duplicate spans
matches = re.findall(r'<span class="h">🔍.*?</span>', text)
print(f"Total Breakdown header occurrences now: {len(matches)}")
for m in matches:
    print(" ", m)

with open("script.html", "w") as f:
    f.write(text)

print("script.html duplicates cleaned successfully.")
