import re

with open("script.html") as f:
    text = f.read()

# Add CSS for .slide-thumb
css_thumb = '''
.slide-thumb{margin:.6rem 0 1rem 0; border-radius:4px; overflow:hidden; border:1px solid var(--rule); box-shadow:var(--shadow); max-width:28rem}
.slide-thumb img{display:block; width:100%; height:auto; aspect-ratio:330/186; object-fit:cover}
'''
text = text.replace('/* the spoken line', css_thumb + '\n/* the spoken line')

# Print styles for .slide-thumb
print_css_thumb = '''
  .slide-thumb{max-width:85mm; margin:.4rem 0 .6rem 0; box-shadow:none; border-color:#D9E2E2}
'''
text = text.replace('.onscreen{font-size:8.5pt;', print_css_thumb + '\n  .onscreen{font-size:8.5pt;')

# Map beats 01 to 10 to thumbs/slide-01.png .. slide-10.png
for i in range(1, 11):
    num_str = f"{i:02d}"
    thumb_tag = f'''<div class="slide-thumb"><img src="thumbs/slide-{num_str}.png" alt="Slide {num_str}"></div>'''
    
    # Insert thumb right after <div class="onscreen">...</div>
    pattern = rf'(<div class="slug"><span class="n">{num_str}</span>.*?<div class="onscreen">.*?</div>)'
    match = re.search(pattern, text, re.DOTALL)
    if match:
        text = re.sub(pattern, rf'\1\n  {thumb_tag}', text, count=1, flags=re.DOTALL)
        print(f"Embedded thumb for Beat {num_str}")
    else:
        print(f"Beat {num_str} not matched")

with open("script.html", "w") as f:
    f.write(text)

print("script.html updated with slide thumbnails.")
