{
  "base": "{{ mode }}",
  "colors": {
    "window": "{{ background }}",
    "panel": "{{ mix background foreground 5% }}",
    "surface": "{{ mix background foreground 12% }}",
    "surface_hover": "{{ mix background foreground 17% }}",
    "text": "{{ foreground }}",
    "secondary": "{{ mix background foreground 70% }}",
    "dim": "{{ mix background foreground 50% }}",
    "accent": "{{ accent }}",
    "on_accent": "{{ background }}",
    "danger": "{{ red }}",
    "warning": "{{ yellow }}",
    "bubble_in": "{{ mix background foreground 14% }}",
    "bubble_out": "{{ mix background accent 32% }}",
    "link": "{{ accent }}",
    "ok": "{{ green }}"
  }
}
