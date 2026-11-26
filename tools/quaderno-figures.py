#!/usr/bin/env python3
"""Draw the figures of the quaderno (docs/quaderno/img/*.svg).

Every figure is drawn here, from the numbers written here, so that a
figure can be brought up to date when a step is closed: change the
number, run the script, commit the SVG that changed.

    tools/quaderno-figures.py            draw them all
    tools/quaderno-figures.py NAME ...   only these (file names, no .svg)

No dependency beyond the standard library. The text of the figures is
Italian, like the quaderno; names and comments are English, like the
code. Colours: text in two inks, four hues for four kinds of thing, each
always named beside its mark, never told by colour alone.
"""

import math
import pathlib
import sys

OUT = pathlib.Path(__file__).resolve().parent.parent / "docs" / "quaderno" / "img"

INK = "#191919"
INK2 = "#666666"
GRID = "#d9d9d9"
SURFACE = "#ffffff"
SOFT = "#f4f4f4"
BLUE, BLUE_TINT = "#257de0", "#cce3fd"
ORANGE, ORANGE_TINT = "#f47230", "#fde0d2"
AQUA, AQUA_TINT = "#00b894", "#c9f0e8"
YELLOW, YELLOW_TINT = "#f2a600", "#fcebc4"
CRITICAL = "#dc3c3d"
FONT = "system-ui, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif"


def esc(text):
    return str(text).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


class Figure:
    """An SVG on a white card, with what the drawings below need."""

    def __init__(self, width, height, label):
        self.width, self.height, self.label = width, height, label
        self.parts = []

    def add(self, text):
        self.parts.append(text)

    def rect(self, x, y, w, h, fill=SOFT, stroke=None, rx=4, width=1, dash=None):
        extra = f' stroke="{stroke}" stroke-width="{width}"' if stroke else ""
        if dash:
            extra += f' stroke-dasharray="{dash}"'
        self.add(
            f'<rect x="{x:g}" y="{y:g}" width="{w:g}" height="{h:g}" rx="{rx:g}" fill="{fill}"{extra}/>'
        )

    def text(self, x, y, text, size=12, anchor="start", fill=INK, weight=None, italic=False):
        extra = f' font-weight="{weight}"' if weight else ""
        if italic:
            extra += ' font-style="italic"'
        self.add(
            f'<text x="{x:g}" y="{y:g}" font-size="{size:g}" text-anchor="{anchor}" fill="{fill}"{extra}>{esc(text)}</text>'
        )

    def lines(self, x, y, texts, size=12, anchor="start", fill=INK, gap=None, weight=None):
        """Several lines of text, the first at y."""
        gap = gap or size * 1.3
        for index, text in enumerate(texts):
            self.text(x, y + index * gap, text, size, anchor, fill, weight)

    def line(self, x1, y1, x2, y2, stroke=INK2, width=1, dash=None, arrow=False):
        extra = f' stroke-dasharray="{dash}"' if dash else ""
        if arrow:
            extra += ' marker-end="url(#arrow)"'
        self.add(
            f'<line x1="{x1:g}" y1="{y1:g}" x2="{x2:g}" y2="{y2:g}" stroke="{stroke}" stroke-width="{width:g}"{extra}/>'
        )

    def path(self, d, stroke=INK2, width=1, fill="none", dash=None, arrow=False):
        extra = f' stroke-dasharray="{dash}"' if dash else ""
        if arrow:
            extra += ' marker-end="url(#arrow)"'
        self.add(f'<path d="{d}" stroke="{stroke}" stroke-width="{width:g}" fill="{fill}"{extra}/>')

    def circle(self, x, y, r, fill, ring=True):
        extra = f' stroke="{SURFACE}" stroke-width="2"' if ring else ""
        self.add(f'<circle cx="{x:g}" cy="{y:g}" r="{r:g}" fill="{fill}"{extra}/>')

    def box(self, x, y, w, h, texts, fill=SOFT, stroke=GRID, size=12, weight=None):
        """A box with its text in the middle, one line or more."""
        self.rect(x, y, w, h, fill, stroke)
        texts = [texts] if isinstance(texts, str) else texts
        gap = size * 1.3
        first = y + h / 2 + size * 0.35 - (len(texts) - 1) * gap / 2
        for index, text in enumerate(texts):
            self.text(
                x + w / 2,
                first + index * gap,
                text,
                size,
                "middle",
                weight=weight if index == 0 else None,
            )

    def bar(self, x, y, w, h, fill, horizontal=True):
        """A bar with its data end rounded and its base square."""
        r = min(4, w / 2, h / 2)
        if horizontal:
            d = f"M{x:g},{y:g} h{w - r:g} a{r},{r} 0 0 1 {r},{r} v{h - 2 * r:g} a{r},{r} 0 0 1 {-r},{r} h{-(w - r):g} z"
        else:
            d = f"M{x:g},{y + h:g} v{-(h - r):g} a{r},{r} 0 0 1 {r},{-r} h{w - 2 * r:g} a{r},{r} 0 0 1 {r},{r} v{h - r:g} z"
        self.add(f'<path d="{d}" fill="{fill}"/>')

    def save(self, name):
        OUT.mkdir(parents=True, exist_ok=True)
        head = (
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {self.width} {self.height}" '
            f'width="{self.width}" height="{self.height}" role="img" aria-label="{esc(self.label)}" '
            f'font-family="{FONT}">\n'
            f"<title>{esc(self.label)}</title>\n"
            '<defs><marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" '
            f'markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="{INK2}"/>'
            "</marker></defs>\n"
            f'<rect x="0.5" y="0.5" width="{self.width - 1}" height="{self.height - 1}" rx="8" '
            f'fill="{SURFACE}" stroke="{GRID}"/>\n'
        )
        (OUT / f"{name}.svg").write_text(head + "\n".join(self.parts) + "\n</svg>\n")


# ---------------------------------------------------------------- diagrams


def domini_285k():
    """The 24 CPUs of the 285K and the domain of each."""
    f = Figure(760, 300, "I 24 processori del 285K e il dominio di ognuno")
    f.text(24, 34, "I 24 processori del 285K e il loro dominio", 15, weight=600)
    domains = {
        "live": (BLUE_TINT, BLUE),
        "anticipativo": (ORANGE_TINT, ORANGE),
        "system": (AQUA_TINT, AQUA),
        "io": (YELLOW_TINT, YELLOW),
    }

    def domain(cpu):
        if 4 <= cpu <= 7:
            return "live"
        if cpu <= 3 or 8 <= cpu <= 15:
            return "anticipativo"
        return "system" if cpu <= 19 else "io"

    f.text(24, 78, "P-core", 12, fill=INK2)
    f.text(24, 94, "veloci", 11, fill=INK2)
    for cpu in range(8):
        fill, stroke = domains[domain(cpu)]
        f.box(96 + cpu * 40, 62, 36, 36, str(cpu), fill, stroke, 13)
    f.text(24, 140, "E-core", 12, fill=INK2)
    f.text(24, 156, "efficienti", 11, fill=INK2)
    for cpu in range(8, 24):
        fill, stroke = domains[domain(cpu)]
        f.box(96 + (cpu - 8) * 40, 124, 36, 36, str(cpu), fill, stroke, 13)
    # Braces under the groups, each with its name and its job.
    notes = [
        (96, 4, "live", "4–7: il ciclo audio,", "e nient'altro", 62 + 36, 4),
        (96, 12, "anticipativo", "0–3 e 8–15: il calcolo che", "non scade entro il periodo", None, None),
        (416, 4, "system", "16–19: il sistema,", "il cane da guardia", None, None),
        (576, 4, "io", "20–23: le interruzioni", "delle periferiche", None, None),
    ]
    y = 206
    x = 24
    for _, _, name, first, second, _, _ in notes:
        fill, stroke = domains[name]
        f.rect(x, y, 14, 14, fill, stroke, 3)
        f.text(x + 22, y + 12, name, 13, weight=600)
        f.text(x + 22, y + 32, first, 11.5, fill=INK2)
        f.text(x + 22, y + 48, second, 11.5, fill=INK2)
        x += 184
    f.save("domini-285k")


def periodo_e_buffer():
    """Sample, frame, period and buffer for the shape 96:3."""
    f = Figure(760, 330, "Frame, periodo e buffer con la forma 96:3 a 48 kHz")
    f.text(24, 34, "Frame, periodo e buffer: la forma 96:3 a 48 kHz", 15, weight=600)
    # One period blown up into frames.
    f.text(24, 72, "Un periodo: 96 frame, 2 ms", 12, fill=INK2)
    for index in range(24):
        f.rect(24 + index * 9, 82, 7, 26, BLUE_TINT, BLUE, 1.5)
    f.text(246, 100, "…", 14, fill=INK2)
    f.text(270, 92, "un frame: un campione per canale,", 11.5, fill=INK2)
    f.text(270, 107, "nello stesso istante (1/48 000 di secondo)", 11.5, fill=INK2)
    # The buffer of the output: three periods.
    f.text(24, 152, "Il buffer di uscita: tre periodi, 288 frame, 6 ms", 12, fill=INK2)
    x0, y0, w, h = 150, 166, 142, 56
    names = ["sta suonando", "pronto", "pronto"]
    for index in range(3):
        f.box(x0 + index * (w + 4), y0, w, h, [f"periodo {index + 1}", names[index]], BLUE_TINT, BLUE)
    f.box(x0 + 3 * (w + 4), y0, w, h, ["il prossimo", "lo scrive il motore"], SOFT, GRID)
    f.box(24, y0, 104, h, ["la scheda", "consuma da qui"], YELLOW_TINT, YELLOW)
    f.line(148, y0 + h / 2, 130, y0 + h / 2, arrow=True)
    f.line(x0 + 3 * (w + 4) - 2, y0 + h + 22, x0 + 3 * (w + 4) - 2, y0 + h + 4, arrow=True)
    f.text(x0 + 3 * (w + 4) - 2, y0 + h + 38, "il motore scrive qui, un periodo ogni 2 ms", 11.5, "middle", INK2)
    # What the picture says.
    f.lines(
        24,
        290,
        [
            "Ogni campione che il motore scrive aspetta in coda dietro a ciò che c'è già: più il buffer è lungo, più tempo ha",
            "il motore prima che la scheda resti senza (xrun), e più tardi si sente il suono. Con due periodi non si regge.",
        ],
        11.5,
        fill=INK2,
    )
    f.save("periodo-e-buffer")


def ciclo():
    """One cycle of a live thread on the time axis."""
    f = Figure(760, 330, "Un ciclo del motore lungo un periodo di 2 ms")
    f.text(24, 34, "Un ciclo del motore: due fasi in un periodo di 2 ms", 15, weight=600)
    f.text(24, 54, "Non in scala: il risveglio dura microsecondi, il calcolo fino a 1,85 ms.", 11.5, fill=INK2)
    x0, x1, y = 40, 720, 120
    h = 44
    # Phases: wait | wake-up | compute (input, nodes, output) | wait.
    parts = [
        (40, 110, "attesa", SOFT, GRID),
        (150, 46, "", YELLOW_TINT, YELLOW),
        (196, 70, "ingresso", BLUE_TINT, BLUE),
        (270, 154, "i nodi del grafo", BLUE_TINT, BLUE),
        (428, 66, "uscita", BLUE_TINT, BLUE),
        (498, 182, "attesa", SOFT, GRID),
        (680, 46, "", YELLOW_TINT, YELLOW),
    ]
    for x, w, name, fill, stroke in parts:
        f.box(x, y, w - 4, h, name, fill, stroke)
    f.text(171, y + h + 18, "risveglio", 11.5, "middle", INK2)
    f.text(171, y + h + 33, "3–8 µs", 11.5, "middle", INK2)
    # Brackets for the two phases.
    f.line(198, y + h + 14, 490, y + h + 14, BLUE, 2)
    f.text(344, y + h + 32, "fase di calcolo: nessun ingresso nel kernel", 12, "middle")
    f.line(498, y + h + 14, 676, y + h + 14, INK2, 2)
    f.text(587, y + h + 32, "fase di attesa:", 12, "middle")
    f.text(587, y + h + 47, "una chiamata bloccante", 12, "middle")
    # Instants.
    marks = [
        (150, "periodo dovuto", "(lo dice la scheda)", INK2),
        (590, "bilancio", "non si comincia un altro nodo", ORANGE),
        (680, "scadenza", "", CRITICAL),
    ]
    for x, first, second, colour in marks:
        f.line(x, 84, x, y + h + 4, colour, 1.5, "4 3")
    f.text(150, 78, "periodo dovuto", 11.5, "middle")
    f.text(590, 78, "bilancio", 11.5, "middle")
    f.text(680, 78, "scadenza", 11.5, "middle")
    f.line(592, 100, 678, 100, INK2, 1)
    f.text(635, 95, "margine: 150 µs", 10.5, "middle", INK2)
    # The axis.
    f.line(x0, 250, x1, 250, GRID, 1)
    for x, name in ((150, "0"), (680, "2 ms: il periodo dopo")):
        f.line(x, 246, x, 254, INK2)
        f.text(x, 270, name, 11.5, "middle", INK2)
    f.lines(
        24,
        296,
        [
            "Dopo il bilancio non si comincia un altro nodo: se ne restano, il ciclo è «tagliato». Meglio meno calcolo che un xrun.",
            "La scadenza è il più vicino di tre istanti (passo P1.7b); nel caso semplice è la fine del periodo.",
        ],
        11.5,
        fill=INK2,
    )
    f.save("ciclo")


def giro():
    """The round trip of a sample: by the cable and inside the interface."""
    f = Figure(760, 360, "Il giro di un campione: per il cavo e dentro l'interfaccia")
    f.text(24, 34, "Il giro: da un campione scritto allo stesso campione riletto", 15, weight=600)
    y, h = 70, 50
    f.box(24, 150, 110, 70, ["dawos-engine", "scrive e rilegge"], BLUE_TINT, BLUE, weight=600)
    # The way out, on top.
    top = [
        (170, 130, ["buffer di uscita", "3 periodi: 6 ms"], SOFT, GRID),
        (316, 76, ["USB"], SOFT, GRID),
        (408, 120, ["router", "dell'interfaccia"], YELLOW_TINT, YELLOW),
        (544, 96, ["convertitore", "in analogico"], YELLOW_TINT, YELLOW),
    ]
    for x, w, texts, fill, stroke in top:
        f.box(x, y, w, h, texts, fill, stroke, 11.5)
    # The way back, below.
    yb = 250
    bottom = [
        (170, 130, ["buffer di ingresso", "1 periodo: 2 ms"], SOFT, GRID),
        (316, 76, ["USB"], SOFT, GRID),
        (408, 120, ["router", "dell'interfaccia"], YELLOW_TINT, YELLOW),
        (544, 96, ["convertitore", "in digitale"], YELLOW_TINT, YELLOW),
    ]
    for x, w, texts, fill, stroke in bottom:
        f.box(x, yb, w, h, texts, fill, stroke, 11.5)
    # Arrows of the way out.
    f.path(f"M79,150 V{y + h / 2} H168", arrow=True)
    for a, b in ((300, 314), (392, 406), (528, 542)):
        f.line(a, y + h / 2, b, y + h / 2, arrow=True)
    # The cable.
    f.path(f"M640,{y + h / 2} H700 V{yb + h / 2} H642", ORANGE, 2, arrow=True)
    f.text(708, 180, "cavo", 12, "start")
    f.text(708, 196, "3 → 3", 11.5, "start", INK2)
    # Inside the interface, without the converters.
    f.path(f"M468,{y + h} V{yb - 2}", AQUA, 2, dash="5 3", arrow=True)
    f.text(476, 176, "dentro l'interfaccia", 12)
    f.text(476, 192, "5 → 5, 6 → 6: bit per bit", 11.5, fill=INK2)
    # Arrows of the way back.
    for a, b in ((542, 530), (406, 394), (314, 302)):
        f.line(a, yb + h / 2, b, yb + h / 2, arrow=True)
    f.path(f"M168,{yb + h / 2} H79 V222", arrow=True)
    f.lines(
        24,
        330,
        [
            "Per il cavo il giro si misura con le raffiche (ci sono i convertitori); dentro l'interfaccia con il conteggio, al campione.",
            "Giro = buffer + una costante del driver: 0,4–0,5 ms sulla seconda generazione, 3,4–3,7 ms sulla quarta.",
        ],
        11.5,
        fill=INK2,
    )
    f.save("giro")


def usb_blocchi():
    """Microframes, blocks of a millisecond, and the period told early."""
    f = Figure(760, 334, "L'audio USB arriva al driver a blocchi di un millisecondo")
    f.text(24, 34, "L'audio USB arriva al driver a blocchi di un millisecondo", 15, weight=600)
    x0, unit = 60, 20
    # Microframes: 32 of them, 4 ms.
    f.text(24, 76, "pacchetti USB: uno ogni 125 µs", 11.5, fill=INK2)
    for index in range(32):
        f.rect(x0 + index * unit, 86, unit - 3, 18, YELLOW_TINT, YELLOW, 2)
    # Blocks of eight.
    f.text(24, 130, "ciò che il driver riceve: blocchi di otto pacchetti, 1 ms", 11.5, fill=INK2)
    for index in range(4):
        f.box(x0 + index * 8 * unit, 140, 8 * unit - 3, 26, "1 ms", SOFT, GRID, 11.5)
    # Periods of 2 ms and when they are told.
    for index in range(2):
        f.box(x0 + index * 16 * unit, 204, 16 * unit - 3, 26, f"periodo {index + 1}", BLUE_TINT, BLUE, 11.5)
    for index in (1, 2):
        x = x0 + index * 16 * unit - 2
        f.line(x, 170, x, 200, BLUE, 1.5, arrow=True)
    # The early tick.
    x = x0 + 3 * 8 * unit - 2
    f.line(x, 170, x, 200, ORANGE, 1.5, "4 3", arrow=True)
    f.text(24, 252, "il periodo del motore: 96 frame, 2 ms, annunciato dal blocco che lo completa (frecce blu)", 11.5, fill=INK2)
    f.line(24, 268, 44, 268, ORANGE, 1.5, "4 3")
    f.text(52, 272, "ogni tanto l'annuncio scatta un blocco prima", 11.5, fill=INK2)
    f.lines(
        24,
        300,
        [
            "L'orologio dell'interfaccia e quello del bus non coincidono: sulla Scarlett di quarta generazione, undici volte all'ora,",
            "un periodo è annunciato un millisecondo prima del solito (A-68). Un periodo che non è multiplo di 1 ms arriva a scatti.",
        ],
        11.5,
        fill=INK2,
    )
    f.save("usb-blocchi")


def k04():
    """What happens, and when, after a thread gets stuck."""
    f = Figure(760, 350, "La protezione K-04: che cosa succede dopo che un thread si blocca")
    f.text(24, 34, "La protezione K-04: dal blocco di un thread al grafo senza il nodo", 15, weight=600)
    f.text(24, 54, "Tempi dal blocco, con periodi da 2 ms. L'asse non è in scala dopo i 12 ms.", 11.5, fill=INK2)
    x0, y = 60, 190

    def at(ms):
        # Linear to 12 ms, then squeezed.
        return x0 + ms * 34 if ms <= 12 else x0 + 12 * 34 + (math.log10(ms) - math.log10(12)) * 150

    f.line(x0, y, 730, y, INK2, 1)
    for ms in (0, 2, 4, 6, 8, 10, 12, 20, 50, 500):
        f.line(at(ms), y - 4, at(ms), y + 4, INK2)
        f.text(at(ms), y + 20, f"{ms} ms" if ms == 500 else f"{ms}", 11, "middle", INK2)
    # The thread stuck, period by period.
    f.box(at(0), y - 44, at(6) - at(0) - 3, 30, "il thread non ritorna: tre periodi", ORANGE_TINT, ORANGE, 11.5)
    # Events above the axis.
    events = [
        (0, 92, "il blocco", ORANGE),
        (6.97, 76, "il cane da guardia se ne accorge", BLUE),
        (6.99, 104, "processore libero, nodo abbandonato: silenzio", BLUE),
        (8.0, 132, "la riserva è al lavoro", BLUE),
        (16.14, 160, "il grafo senza il nodo è in uso", AQUA),
    ]
    for ms, top, name, colour in events:
        f.line(at(ms), top + 6, at(ms), y - (48 if ms < 6 else 0), colour, 1.5)
        f.circle(at(ms), y if ms >= 6 else y - 44, 4, colour)
        f.text(at(ms) + 6, top, name, 11.5)
    # Limits below the axis.
    limits = [(8.5, "accorgersene"), (15, "processore libero"), (50, "riserva al lavoro"), (500, "grafo senza il nodo")]
    f.text(24, y + 52, "limiti", 11.5, fill=INK2)
    for index, (ms, name) in enumerate(limits):
        yy = y + 52 + index * 20
        f.line(at(ms), y + 26, at(ms), yy - 4, CRITICAL, 1, "3 3")
        f.text(at(ms) - 6, yy, f"{name}: entro {str(ms).replace('.', ',')} ms", 11.5, "end")
    f.lines(
        24,
        322,
        [
            "Misurato sul 285K con un nodo del grafo che non ritorna (P1.7f): visto dopo 6,97 ms, riserva al lavoro",
            "dopo 8,00 ms, grafo senza il nodo in uso dopo 16,14 ms. Nessun campione perso sugli altri thread.",
        ],
        11.5,
        fill=INK2,
    )
    f.save("k04")


def grafo_thread():
    """A graph on four live threads: the count handed from one to the next."""
    f = Figure(760, 378, "Un grafo su quattro thread live: il conteggio passato di mano")
    f.text(24, 34, "Un grafo su quattro thread: ogni nodo ha il suo thread", 15, weight=600)
    f.text(24, 54, "relay.graph: il conteggio entra dall'ingresso 5, passa da tre thread, esce sull'uscita 6.", 11.5, fill=INK2)
    lanes = 4
    y0, lane_h = 74, 56
    for lane in range(lanes):
        y = y0 + lane * lane_h
        f.rect(24, y, 712, lane_h - 6, SOFT if lane % 2 == 0 else SURFACE, GRID, 4)
        f.text(36, y + 22, f"thread {lane}", 12, weight=600)
        f.text(36, y + 38, "tiene la scheda" if lane == 0 else "segue la scheda", 10.5, fill=INK2)

    def node(lane, x, name, kind, colour=(BLUE_TINT, BLUE), w=78):
        y = y0 + lane * lane_h + 8
        f.box(x, y, w, 34, [name, kind], colour[0], colour[1], 11)
        return x, y + 17, w

    busy = (SOFT, INK2)
    in5 = node(0, 150, "in5", "ingresso")
    hop1 = node(1, 262, "hop1", "guadagno")
    hop2 = node(2, 374, "hop2", "guadagno")
    hop3 = node(3, 486, "hop3", "guadagno")
    out6 = node(0, 598, "out6", "uscita")
    for lane in range(4):
        node(lane, 150 if lane else 262, f"busy{lane}", "lavoro", busy)
    chain = [in5, hop1, hop2, hop3, out6]
    for a, b in zip(chain, chain[1:]):
        ax, ay, aw = a
        bx, by, _ = b
        mid = (ax + aw + bx) / 2
        f.path(f"M{ax + aw},{ay} H{mid} V{by} H{bx - 2}", ORANGE, 1.8, arrow=True)
    f.text(374 + 39, y0 + 4 * lane_h + 18, "ogni freccia arancione è un passaggio tra thread: chi riceve aspetta girando a vuoto,", 11.5, "middle", INK2)
    f.text(374 + 39, y0 + 4 * lane_h + 34, "senza entrare nel kernel, finché il nodo prima ha scritto il suo segno per questo ciclo", 11.5, "middle", INK2)
    f.text(24, 360, "Misurato (P1.7c): un'attesa dura meno di un microsecondo; 8 640 172 campioni al loro posto, nessuno fuori posto.", 11.5, fill=INK2)
    f.save("grafo-thread")


def scambio():
    """A graph in the place of another: every thread changes at the same cycle."""
    f = Figure(760, 356, "Lo scambio del grafo: tutti i thread cambiano allo stesso ciclo")
    f.text(24, 34, "Lo scambio del grafo: tutti i thread cambiano allo stesso ciclo", 15, weight=600)
    x0, cw, y0, lh = 150, 66, 80, 34
    cycles = ["c", "c+1", "c+2", "c+3", "c+4", "c+5", "c+6", "c+7"]
    change = 5
    for index, name in enumerate(cycles):
        f.text(x0 + index * cw + cw / 2, y0 - 8, f"ciclo {name}" if index == 0 else name, 11, "middle", INK2)
    for lane in range(4):
        y = y0 + lane * lh
        f.text(36, y + 19, f"thread {lane}", 12)
        for index in range(len(cycles)):
            new = index >= change
            f.box(
                x0 + index * cw,
                y,
                cw - 3,
                lh - 6,
                "nuovo" if new else "vecchio",
                AQUA_TINT if new else BLUE_TINT,
                AQUA if new else BLUE,
                11,
            )
    xc = x0 + change * cw - 2
    f.line(xc, y0 - 22, xc, y0 + 4 * lh + 4, INK, 1.5, "4 3")
    f.text(xc, y0 + 4 * lh + 20, "il ciclo nominato", 11.5, "middle")
    # The control thread.
    y = y0 + 4 * lh + 44
    f.text(36, y + 19, "thread del giro", 12)
    f.box(x0, y, 2 * cw - 3, lh - 6, "compila e costruisce", SOFT, GRID, 11)
    f.box(x0 + 2 * cw, y, 3 * cw - 3, lh - 6, "offre il grafo: «dal ciclo c+5»", SOFT, GRID, 11)
    f.box(x0 + 5 * cw, y, 3 * cw - 3, lh - 6, "tutti passati: libera il vecchio", SOFT, GRID, 11)
    f.lines(
        24,
        318,
        [
            "Un thread guarda quale grafo eseguire una volta sola, a inizio ciclo, e lo tiene fino alla fine: nessuno cambia a metà.",
            "Un nodo con lo stesso nome e gli stessi parametri prosegue con il suo stato: 179 scambi, nessun campione perso (P1.7e).",
        ],
        11.5,
        fill=INK2,
    )
    f.save("scambio")


def carico():
    """The reference load: one stereo instance in a vector, stages and trims."""
    f = Figure(760, 330, "Il carico di riferimento: un'istanza stereo in un registro, stadi e ritocchi")
    f.text(24, 34, "Il carico di riferimento: filtri veri, misurati", 15, weight=600)
    # The register of eight lanes.
    f.text(24, 70, "Un registro da 256 bit: otto numeri, cioè un'istanza stereo divisa in quattro bande", 11.5, fill=INK2)
    for lane in range(8):
        left = lane < 4
        f.box(
            24 + lane * 56,
            80,
            52,
            30,
            f"{'S' if left else 'D'} banda {lane % 4 + 1}",
            BLUE_TINT if left else AQUA_TINT,
            BLUE if left else AQUA,
            10.5,
        )
    f.text(486, 92, "S: canale sinistro", 11.5, fill=INK2)
    f.text(486, 108, "D: canale destro", 11.5, fill=INK2)
    # The cascade.
    y = 150
    f.text(24, y - 6, "Per ogni blocco: tanti stadi quanti ne servono, poi i ritocchi", 11.5, fill=INK2)
    x = 24
    f.box(x, y, 84, 44, ["ingresso", "S e D"], SOFT, GRID, 11.5)
    x += 98
    for index in range(4):
        f.box(x, y, 84, 44, [f"stadio {index + 1}", "passa-tutto"], ORANGE_TINT, ORANGE, 11.5)
        f.line(x - 14, y + 22, x - 2, y + 22, arrow=True)
        x += 98
    f.text(x - 12, y + 12, "…", 14, fill=INK2)
    x += 14
    for index in range(2):
        f.box(x, y, 70, 44, [f"ritocco {index + 1}", "× costante"], YELLOW_TINT, YELLOW, 11.5)
        f.line(x - 14, y + 22, x - 2, y + 22, arrow=True)
        x += 84
    f.box(x, y, 60, 44, ["uscita", "S e D"], SOFT, GRID, 11.5)
    f.line(x - 14, y + 22, x - 2, y + 22, arrow=True)
    # Costs and classes.
    y = 228
    f.text(24, y, "Costi misurati sul 285K, per frame, a 96 kHz (P1.7d)", 12, weight=600)
    f.text(24, y + 20, "parte fissa 1,92 ns   ·   uno stadio 1,845 ns   ·   un ritocco 0,207 ns", 12)
    rows = [
        ("classe leggera, 10 ns", "4 stadi e 2 ritocchi", "9,95 ns"),
        ("classe media, 40 ns", "20 stadi e 4 ritocchi", "40,17 ns"),
        ("classe pesante, 150 ns", "80 stadi", "149,66 ns"),
    ]
    for index, (name, made, got) in enumerate(rows):
        yy = y + 46 + index * 18
        f.text(24, yy, name, 11.5)
        f.text(220, yy, made, 11.5, fill=INK2)
        f.text(400, yy, f"misurato {got}", 11.5, fill=INK2)
    f.save("carico")


def dove_gira():
    """Where a test runs: the PC, the virtual machine, the 285K."""
    f = Figure(760, 330, "Dove gira una prova: PC di sviluppo, macchina virtuale, 285K")
    f.text(24, 34, "Dove gira una prova: tre luoghi, sempre in quest'ordine", 15, weight=600)
    y, h = 64, 150
    places = [
        (24, "1. PC di sviluppo", ["make engine-test", "make lint", "make image"], "la logica del codice,", "la forma, l'immagine", SOFT, GRID),
        (272, "2. Macchina virtuale", ["make qemu-test", "make qemu-audio-test"], "l'immagine parte, il motore", "suona sulla scheda di software", SOFT, GRID),
        (520, "3. 285K", ["make remote-update (dal PC)", "dawos-audio-test card", "tools/graph-threads.sh"], "le misure che contano:", "i tempi, la scheda vera", BLUE_TINT, BLUE),
    ]
    for x, name, commands, first, second, fill, stroke in places:
        f.rect(x, y, 216, h, fill, stroke, 6)
        f.text(x + 14, y + 26, name, 13, weight=600)
        for index, command in enumerate(commands):
            f.add(
                f'<text x="{x + 14}" y="{y + 52 + index * 18}" font-size="11.5" fill="{INK}" '
                f'font-family="ui-monospace, Menlo, Consolas, monospace">{esc(command)}</text>'
            )
        f.text(x + 14, y + h - 30, first, 11.5, fill=INK2)
        f.text(x + 14, y + h - 14, second, 11.5, fill=INK2)
    f.line(242, y + 60, 270, y + 60, arrow=True)
    f.line(490, y + 60, 518, y + 60, arrow=True)
    # The way back of the measures.
    yb = y + h + 44
    f.path(f"M628,{y + h + 2} V{yb} H134 V{y + h + 4}", ORANGE, 1.8, arrow=True)
    f.text(380, yb - 8, "make measure-fetch: i file delle misure tornano nel repository, controllati", 11.5, "middle")
    f.lines(
        24,
        yb + 34,
        [
            "Se una prova fallisce in un luogo non si passa al successivo. La macchina virtuale prova la logica, non i tempi:",
            "con KVM gira alla velocità del processore, senza è emulata e il cane da guardia prende per bloccati i thread lenti.",
        ],
        11.5,
        fill=INK2,
    )
    f.save("dove-gira")


def nodo_abbandonato():
    """What a stuck thread holds, and what the others see: before, during, after."""
    f = Figure(760, 380, "Il nodo abbandonato: ciò che il thread bloccato si tiene")
    f.text(24, 34, "Il nodo abbandonato: ciò che il thread bloccato si tiene", 15, weight=600)
    stages = [
        (24, "1. Il nodo non ritorna", "il thread tiene il suo stato e i suoi buffer"),
        (272, "2. Abbandonato", "silenzio per chi leggeva quei buffer"),
        (520, "3. Il grafo senza il nodo", "ciò che gli stava accanto ritorna"),
    ]
    for stage, (x, name, note) in enumerate(stages):
        f.rect(x, 56, 216, 256, SURFACE, GRID, 6)
        f.text(x + 12, 78, name, 12.5, weight=600)
        f.text(x + 12, 94, note, 10.5, fill=INK2)
        held = stage < 2
        lost = stage == 1
        # The source and its buffer.
        f.box(x + 12, 112, 82, 32, "sorgente", BLUE_TINT, BLUE, 11)
        buffer_fill = (ORANGE_TINT, ORANGE) if held else (SOFT, GRID)
        f.box(x + 12, 168, 82, 28, "buffer", buffer_fill[0], buffer_fill[1], 11)
        f.line(x + 53, 146, x + 53, 166, arrow=True)
        # The stuck node and the node beside it.
        if stage < 2:
            f.box(x + 12, 226, 82, 34, ["bloccato", "(thread 2)"], ORANGE_TINT, ORANGE, 11)
            f.line(x + 40, 198, x + 40, 224, ORANGE, 1.8, arrow=True)
            f.box(x + 12, 272, 82, 28, "buffer", ORANGE_TINT, ORANGE, 11)
            f.line(x + 53, 261, x + 53, 271, ORANGE, 1.8)
        else:
            f.rect(x + 12, 226, 82, 34, SURFACE, GRID, 4, dash="4 3")
            f.text(x + 53, 247, "tolto", 11, "middle", INK2)
        f.box(x + 122, 226, 82, 34, ["accanto", "(thread 3)"], BLUE_TINT, BLUE, 11)
        dash = "4 3" if stage < 2 else None
        f.path(f"M{x + 94},182 H{x + 163} V224", INK2, 1.4, dash=dash, arrow=True)
        label = ["trova occupato", "(graph_refused)"] if stage == 0 else (["sente silenzio"] if lost else ["sente la sorgente"])
        f.lines(x + 106, 150, label, 10.5, fill=INK2)
    f.rect(24, 328, 14, 14, ORANGE_TINT, ORANGE, 3)
    f.text(46, 340, "tenuto dal thread bloccato: in Rust sicuro non gli si può togliere", 11.5, fill=INK2)
    f.text(24, 364, "Misurato (P1.7f): dal blocco al punto 2 passano 6,97 ms; il punto 3 arriva 16,14 ms dopo il blocco.", 11.5, fill=INK2)
    f.save("nodo-abbandonato")


# ------------------------------------------------------------------ charts


def comma(value, digits=2):
    return f"{value:.{digits}f}".replace(".", ",")


def latenza():
    """The round trip by the cable, by rate, on the two interfaces."""
    f = Figure(760, 330, "La latenza del giro per il cavo sulle due interfacce, contro il limite di 10 ms")
    f.text(24, 34, "La latenza del giro per il cavo, forma 96:3", 15, weight=600)
    f.text(24, 54, "Dal minimo al massimo su cento avvii per frequenza, in millisecondi (P1.5 e P1.12c)", 11.5, fill=INK2)
    data = [
        ("44,1 kHz", (7.37, 7.84), (9.66, 10.93)),
        ("48 kHz", (6.00, 7.27), (9.21, 10.23)),
        ("96 kHz", (5.57, 6.71), (8.71, 9.72)),
        ("192 kHz", (5.46, 6.58), (8.55, 9.67)),
    ]
    x0, x1 = 110, 700

    def at(ms):
        return x0 + (ms - 4) / (12 - 4) * (x1 - x0)

    top, row = 96, 44
    for tick in range(4, 13, 2):
        f.line(at(tick), top - 10, at(tick), top + 4 * row - 6, GRID)
        f.text(at(tick), top + 4 * row + 10, f"{tick} ms", 11, "middle", INK2)
    f.line(at(10), top - 22, at(10), top + 4 * row - 6, CRITICAL, 1.5)
    f.text(at(10) + 6, top - 14, "limite di R-01: 10 ms", 11.5)
    for index, (rate, second, fourth) in enumerate(data):
        y = top + index * row
        f.text(x0 - 12, y + 18, rate, 12, "end")
        for offset, (low, high), colour in ((0, second, BLUE), (16, fourth, ORANGE)):
            f.rect(at(low), y + offset, at(high) - at(low), 12, colour, rx=4)
            f.text(at(high) + 6, y + offset + 10, f"{comma(low)}–{comma(high)}", 10.5, fill=INK2)
    # The legend.
    y = top + 4 * row + 34
    f.rect(x0, y, 14, 10, BLUE, rx=3)
    f.text(x0 + 20, y + 9, "Scarlett 18i20 di seconda generazione", 11.5)
    f.rect(x0 + 270, y, 14, 10, ORANGE, rx=3)
    f.text(x0 + 290, y + 9, "quarta generazione: oltre il limite a 44,1 e 48 kHz (A-69)", 11.5)
    f.save("latenza")


def thread_1_4():
    """The same graph on one to four threads: expected and measured."""
    f = Figure(760, 340, "Lo stesso grafo su uno, due, tre e quattro thread: previsto e misurato")
    f.text(24, 34, "Lo stesso grafo su 1–4 thread: previsto dal compilatore e misurato", 15, weight=600)
    f.text(24, 54, "tracks.graph a 96 kHz, durata del ciclo in microsecondi; il periodo è di 2000 µs (P1.7e)", 11.5, fill=INK2)
    expected = [287.00, 144.45, 99.08, 75.80]
    measured = [290.00, 147.74, 103.56, 80.61]
    speed = [1.00, 1.96, 2.80, 3.60]
    x0, base, top = 90, 262, 84

    def height(us):
        return us / 300 * (base - top)

    for tick in range(0, 301, 100):
        y = base - height(tick)
        f.line(x0, y, 700, y, GRID)
        f.text(x0 - 8, y + 4, f"{tick}", 11, "end", INK2)
    for index in range(4):
        cx = x0 + 80 + index * 150
        for offset, value, colour in ((-26, expected[index], BLUE), (2, measured[index], ORANGE)):
            f.bar(cx + offset, base - height(value), 24, height(value), colour, horizontal=False)
        f.text(cx - 14, base - height(expected[index]) - 6, comma(expected[index]), 10.5, "middle", INK2)
        f.text(cx + 14, base - height(measured[index]) - 18, comma(measured[index]), 10.5, "middle", INK2)
        f.text(cx, base + 18, f"{index + 1} thread", 12, "middle")
        f.text(cx, base + 34, f"accelerazione {comma(speed[index])}", 11, "middle", INK2)
    y = base + 54
    f.rect(x0, y, 14, 10, BLUE, rx=3)
    f.text(x0 + 20, y + 9, "previsto dal compilatore", 11.5)
    f.rect(x0 + 200, y, 14, 10, ORANGE, rx=3)
    f.text(x0 + 220, y + 9, "misurato: +1,0 % su un thread, +6,3 % su quattro", 11.5)
    f.save("thread-1-4")


def stime():
    """Measured over predicted, for the estimates that had a number."""
    f = Figure(760, 540, "Quanto hanno sbagliato le stime: misurato diviso previsto")
    f.text(24, 34, "Quanto hanno sbagliato le stime: misurato diviso previsto", 15, weight=600)
    f.text(24, 54, "Scala logaritmica: 1 è la stima giusta, 2 il doppio del previsto, 0,5 la metà. Per un intervallo, il suo centro.", 11.5, fill=INK2)
    ours, below = BLUE, ORANGE
    data = [
        ("Grafo su un thread (P1.7c)", 289.38 / 290.16, ours, "289,38 µs contro 290,16"),
        ("Giro del conteggio su quattro thread (P1.7c)", 1.0, ours, "916 campioni, come calcolato"),
        ("Giro attraverso lo scambio (P1.7e)", 1.0, ours, "1012 campioni, come calcolato"),
        ("Grafo su quattro thread (P1.7e)", 80.61 / 75.80, below, "80,61 µs contro 75,80"),
        ("Uno stadio rispetto al nucleo in C (P1.7d)", 1.845 / 1.67, below, "1,845 ns contro 1,67"),
        ("Potenza della fase di carico pieno (P0.7)", 120 / 160, below, "120 W contro 160"),
        ("Parte fissa di un carico (P1.7d)", 1.92 / 3.0, below, "1,92 ns contro 2,5–3,5"),
        ("Gradi per watt (P0.4b)", 0.30 / 0.5, below, "0,30 °C/W contro 0,5"),
        ("Un ritocco (P1.7d)", 0.207 / 0.4, below, "0,207 ns contro 0,3–0,5"),
        ("Convertitori e cavo (P1.5)", 0.6 / 2.0, below, "0,6 ms contro 1–3"),
        ("Costruire un grafo (P1.7e)", 44 / 150, below, "20 e 68 µs contro 100–200"),
        ("Un guadagno (P1.7d)", 0.06 / 0.3, below, "0,06 ns contro 0,3"),
        ("Risveglio dallo stato C1 (P0.6b)", 0.2 / 1.5, below, "0,1–0,3 µs contro 1–2"),
        ("Una sinusoide (P1.7d)", 7.7 / 4, below, "7,7 ns contro 4"),
        ("Un misuratore (P1.7d)", 1.9 / 0.5, below, "1,9 ns contro 0,5"),
        ("Ritardo con il disco cifrato (P0.6d)", 57.75 / 10, below, "57,75 µs contro meno di 10"),
    ]
    x0, x1, top, row = 296, 556, 86, 23

    def at(ratio):
        return x0 + (math.log10(ratio) + 1) / 2 * (x1 - x0)

    bottom = top + len(data) * row
    f.rect(at(0.5), top - 8, at(2) - at(0.5), bottom - top + 8, SOFT, rx=0)
    f.text(at(1), top - 14, "entro un fattore due", 10.5, "middle", INK2)
    for tick in (0.1, 0.2, 0.5, 1, 2, 5, 10):
        f.line(at(tick), top - 8, at(tick), bottom, INK2 if tick == 1 else GRID, 1.2 if tick == 1 else 1)
        f.text(at(tick), bottom + 16, comma(tick, 1 if tick < 1 else 0), 11, "middle", INK2)
    for index, (name, ratio, colour, note) in enumerate(data):
        y = top + index * row + 8
        f.text(x0 - 16, y + 4, name, 11.5, "end")
        f.line(at(1), y, at(ratio), y, colour, 2)
        f.circle(at(ratio), y, 5, colour)
        f.text(x1 + 14, y + 4, note, 10.5, fill=INK2)
    y = bottom + 40
    f.circle(30, y, 5, ours, ring=False)
    f.text(42, y + 4, "conti su meccanismi nostri, di cui si conosce ogni passaggio", 11.5)
    f.circle(30, y + 20, 5, below, ring=False)
    f.text(42, y + 24, "tempi di ciò che sta sotto: processore, driver, firmware, interfaccia", 11.5)
    f.save("stime")


def risveglio():
    """The worst wake-up delay at four points of the project, against the limit."""
    f = Figure(760, 300, "Il ritardo di risveglio massimo sui processori live, contro il limite di 50 microsecondi")
    f.text(24, 34, "Il ritardo di risveglio peggiore sui processori live", 15, weight=600)
    f.text(24, 54, "Il massimo di ogni misura lunga, in microsecondi; il limite è 50", 11.5, fill=INK2)
    data = [
        ("Campagna di 24 ore (P0.7)", "timer, sotto carico", 20.70),
        ("24 ore con il driver grafico (P1.0)", "timer, sotto carico", 6.71),
        ("Due ore sulla scheda, 2ª generazione (P1.6e)", "il motore, sotto carico", 15.81),
        ("Due ore sulla scheda, 4ª generazione (P1.12c)", "il motore, sotto carico", 14.77),
    ]
    x0, x1, top, row = 330, 690, 86, 44

    def at(us):
        return x0 + us / 55 * (x1 - x0)

    bottom = top + len(data) * row
    for tick in range(0, 51, 10):
        f.line(at(tick), top - 8, at(tick), bottom - 8, GRID)
        f.text(at(tick), bottom + 8, f"{tick}", 11, "middle", INK2)
    f.line(at(50), top - 20, at(50), bottom - 8, CRITICAL, 1.5)
    f.text(at(50) - 6, top - 12, "limite: 50 µs", 11.5, "end")
    for index, (name, how, value) in enumerate(data):
        y = top + index * row
        f.text(x0 - 14, y + 10, name, 11.5, "end")
        f.text(x0 - 14, y + 25, how, 10.5, "end", INK2)
        f.bar(x0, y + 2, at(value) - x0, 20, BLUE)
        f.text(at(value) + 8, y + 16, comma(value), 11.5)
    f.text(24, bottom + 34, "Le prime due misurano un timer sullo stesso processore; le altre due il motore svegliato dalla scheda.", 11, fill=INK2)
    f.save("risveglio")


FIGURES = {
    "domini-285k": domini_285k,
    "periodo-e-buffer": periodo_e_buffer,
    "ciclo": ciclo,
    "giro": giro,
    "usb-blocchi": usb_blocchi,
    "k04": k04,
    "grafo-thread": grafo_thread,
    "scambio": scambio,
    "carico": carico,
    "dove-gira": dove_gira,
    "nodo-abbandonato": nodo_abbandonato,
    "latenza": latenza,
    "thread-1-4": thread_1_4,
    "stime": stime,
    "risveglio": risveglio,
}


def main(names):
    unknown = [name for name in names if name not in FIGURES]
    if unknown:
        sys.exit(f"no figure named {', '.join(unknown)}: one of {', '.join(FIGURES)}")
    for name in names or FIGURES:
        FIGURES[name]()
        print(f"{OUT.relative_to(OUT.parent.parent.parent)}/{name}.svg")


if __name__ == "__main__":
    main(sys.argv[1:])
