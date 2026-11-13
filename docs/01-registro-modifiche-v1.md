# dawOS — Registro delle modifiche del documento 0 verso la versione 1

- **Data**: 13 novembre 2026
- **Origine**: revisione del documento 0 nel prompt della Fase 0
- **Regola di lettura**: il [documento 0](00-revisione-specifiche-fase0.md) resta com'è stato rivisto. Le sezioni riportate qui sotto **sostituiscono per intero** le sezioni con lo stesso nome. Tutto ciò che non è nominato qui resta valido.

Etichette come nel documento 0: _(verificato)_, _(da verificare)_, _(stima)_.

## 1. Elenco delle modifiche

| N. | Sezione del documento 0 | Tipo | Sintesi |
| --- | --- | --- | --- |
| R-01 | Versioni su cui mi baso | Riscritta | Versioni lette dal tag reale di Buildroot e da kernel.org. |
| R-02 | C-04 | Chiusa | Tre partizioni, `shared` da 300 GiB; ripiego definito. |
| R-03 | K-02 | Riscritta | R-04 con carico sintetico; rischio spostato sul dominio anticipativo. |
| R-04 | M-02 | Riscritta | Il freeze delle tracce entra nell'MVP. |
| R-05 | M-11 | Riscritta | GPLv3 con eccezione per i plugin; dipendenze rivalutate. |
| R-06 | M-12 | Chiusa | Mix cuffie con monitoraggio diretto tramite `fcp-server`. |
| R-07 | 2.7 | Aggiunta | Verifica del budget con serializzazione dei segmenti per core. |
| R-08 | 3.2 | Riscritta | Struttura del repository com'è stata costruita. |
| R-09 | 3.3 | Riscritta | Schema del disco con le nuove dimensioni. |
| R-10 | P0.1 | Riscritta | Ambiente su PC di sviluppo separato. |
| R-11 | P0.2 | Riscritta | Passo eseguito; immagine con genimage. |
| R-12 | P0.4 | Riscritta | Installazione da chiavetta e script di scrittura protetto. |
| R-13 | P0.5 | Aggiunta | Aggiornamento con `systemd-sysupdate` dal PC di sviluppo. |
| R-14 | P0.6 | Aggiunta | Esperimenti E-05 (streaming nelle home) ed E-06 (rapporto tra core). |
| R-15 | 3.5 | Corretta | `turbostat` e `ukify` nel tag reale. |
| R-16 | Parte 4 | Aggiornata | D-01…D-09 e D-16 chiuse; le altre rinviate a prima della Fase 1. |

## 2. Sezioni riscritte

### Versioni su cui mi baso

| Componente | Versione | Come lo so |
| --- | --- | --- |
| Buildroot | 2026.02.3, commit `679b9ead7620`, 16 giugno 2026 | Tag letto dal repository ufficiale; è l'ultimo della serie 2026.02 _(verificato)_. |
| Kernel | Linux 6.18.63 | Ultima 6.18.y nell'elenco firmato di kernel.org _(verificato)_. Il kernel predefinito del tag è il 6.19.14, che non usiamo. |
| Toolchain | Bootlin x86-64-v3, glibc, stable 2025.08-1; GCC 14; intestazioni del kernel 5.4 | Opzioni del tag _(verificato)_. |
| systemd | 258.7 | `package/systemd/systemd.mk` del tag _(verificato)_. |
| `ukify` | Disattivato nel pacchetto per la destinazione, attivo nella costruzione per l'host | `-Dukify=disabled` e `-Dukify=enabled` in `systemd.mk` _(verificato)_; come ottenerlo tra gli strumenti host è _da verificare_ nel passo P0.5. |
| Strumenti del kernel | `rtla`, `perf`, `cpupower`, `tmon` e altri; **non** `turbostat` | `package/linux-tools/Config.in` del tag _(verificato)_. |
| VST3 SDK | 3.8.0, licenza MIT | Invariato rispetto al documento 0. |
| `fcp-server` | Ramo `master`, licenza GPL-3.0-or-later | `COPYING` e intestazioni SPDX _(verificato)_. |
| Smithay, wgpu, egui | Non fissate | Si fissano all'inizio della Fase 3. |

Tre affermazioni del documento 0 marcate _da verificare_ sono ora **verificate nei sorgenti del kernel 6.18.63**:

- **C-03**: su x86 il kernel toglie la CPU di avvio dall'insieme `nohz_full` (`kernel/time/tick-sched.c`, messaggio «NO\_HZ: Clearing … from nohz\_full range for timekeeping»). La CPU 0 non può quindi stare nel dominio live.
- **D-03**: la tabella di `dm-mod.create` e `dm-mod.waitfor` accettano `PARTLABEL=` e `PARTUUID=`, perché durante l'avvio la ricerca passa per `early_lookup_bdev` (`drivers/md/dm-table.c`, `drivers/md/dm-init.c`, `block/early-lookup.c`).
- **K-04**: i parametri del _fair server_ per CPU esistono in debugfs (`kernel/sched/debug.c`).

Restano da osservare sul sistema in funzione, nei passi indicati.

### C-04 — XFS con reflink, systemd-homed e cifratura a due livelli

**Stato: chiusa**, vedi [ADR 0004](adr/0004-partizioni-dei-dati.md).

Si adottano tre partizioni di dati: `state` (32 GiB), `shared` (300 GiB), `homes` (il resto). Ogni dato utente è cifrato una volta sola, da homed, con immagini LUKS2 ospitate su `homes`.

Resta un rischio: lo streaming dei progetti attraversa un dispositivo loop e dm-crypt. Lo misura l'esperimento E-05 del passo P0.6, con soglie fissate in anticipo. Se le soglie non sono rispettate, il ripiego è una partizione LUKS dedicata per utente.

### K-02 — R-04 misurabile, al limite sul dominio anticipativo

**Gravità: alta.** Vedi [ADR 0011](adr/0011-r04-carico-sintetico.md).

R-04 è ora definito con un carico sintetico in tre classi (10, 40 e 150 ns per campione su P-core, in proporzione 60, 30 e 10 %; media 33 ns) e passa se ogni dominio resta entro il 60 % di utilizzo. Il requisito è quindi misurabile. I conti _(stime)_ dicono:

| Dominio | Frequenza | Utilizzo stimato | Esito atteso |
| --- | --- | --- | --- |
| Live (4 P-core) | 96 kHz | circa 15 % | Passa con margine |
| Live (4 P-core) | 192 kHz | circa 25–28 % | Passa |
| Anticipativo (4 P-core e 8 E-core) | 96 kHz | 58–72 % più qualche punto | Al limite; con la stima centrale non passa |
| Anticipativo | 192 kHz | 117–145 % | Non passa senza freeze |

L'incertezza sta tutta nel rapporto di prestazioni tra E-core e P-core su codice DSP vettoriale, che non conosco e non ho trovato misurato.

Proposte:

1. misurare quel rapporto già nel passo P0.6 con un banco di prova minimo (esperimento E-06, vedi sotto), invece di scoprirlo nella Fase 2;
2. confermare che la sessione a 96 kHz si misura senza tracce congelate e la variante a 192 kHz con il freeze ammesso (punto aperto dell'ADR 0011);
3. se la misura conferma il superamento, agire nell'ordine: blocchi anticipativi più lunghi e meno salti tra processi; riassegnazione di core al dominio anticipativo; riduzione della sessione di riferimento.

Resta valida l'osservazione sulla memoria: 256 processi host possono occupare da 12 a 50 GB _(stima)_; con il carico sintetico la cifra sarà molto più bassa e non rappresentativa di plugin reali.

### M-02 — Funzioni non nominate nel perimetro dell'MVP

Il **freeze delle tracce entra nell'MVP** (decisione della revisione): una traccia congelata viene resa su file e costa solo lo streaming. È lo strumento con cui l'utente rientra nel budget, e ciò che rende raggiungibile la variante stress di R-04.

Conseguenze da prevedere già nei formati: un comando `track.freeze` e il suo inverso; la resa congelata come risorsa immutabile del progetto, rigenerabile; lo stato «congelata» nel piano, dove la catena della traccia è sostituita da un lettore di file.

Restano da confermare prima della Fase 2, con la domanda D-15: automazione dei parametri, mappa di tempo e metrica, marcatori, punch e registrazione a ciclo con take, gruppi e VCA, mandate e sidechain.

### M-11 — Licenze

**Stato: riscritta**, vedi [ADR 0010](adr/0010-licenza-gplv3-eccezione-plugin.md).

dawOS è rilasciato sotto **GPLv3** (`GPL-3.0-only`) con un permesso supplementare secondo la sezione 7, la _dawOS Plugin Exception_: è consentito caricare ed eseguire plugin con qualunque licenza attraverso le interfacce plugin di dawOS. Il testo è nel file `LICENSE` e nell'ADR.

Valutazione delle dipendenze con la nuova licenza:

| Componente | Licenza | Esito |
| --- | --- | --- |
| VST3 SDK 3.8, CLAP | MIT | Compatibili. |
| wgpu, egui, Smithay, crate Rust comuni | MIT o Apache-2.0 | Compatibili; Apache-2.0 è compatibile con la GPLv3. |
| alsa-lib, libsystemd, glibc | LGPL-2.1-or-later | Compatibili con collegamento dinamico. |
| `fcp-server` | GPL-3.0-or-later _(verificato)_ | Compatibile. Usato come processo separato; il suo codice è riusabile in dawOS, ma **non** in un processo che carica plugin. |
| restic | BSD-2-Clause | Processo separato, nessun vincolo. |
| Kernel, Buildroot | GPL-2.0 | Nessun vincolo sul codice in spazio utente né sull'immagine. |
| Librerie solo GPL-2.0 | GPL-2.0-only | Non collegabili: da escludere o usare come processi separati. |

Regola nuova e importante: **nei processi che caricano plugin entrano solo dipendenze permissive o LGPL**. L'eccezione vale per il codice di chi l'ha concessa; una libreria GPL di terzi dentro `dawos-plughost` renderebbe non distribuibile la combinazione con un plugin proprietario.

I punti che richiedono un parere legale sono otto, elencati nell'ADR. I due più urgenti: la tenuta delle definizioni di «Plugin» e «Plugin Interface», e la regola per i contributi di terzi, che va fissata prima del primo contributo esterno.

### M-12 — Monitoraggio diretto tramite il mixer della Scarlett

**Stato: chiusa**, vedi [ADR 0008](adr/0008-fcp-server-processo-separato.md).

Il modulo Mix cuffie userà il monitoraggio diretto del mixer interno della Scarlett, pilotato tramite `fcp-server`. Dipendenze future che ne derivano:

- il modulo Mix cuffie dipende da `fcp-server` e dai controlli ALSA che pubblica; senza la Scarlett il modulo ripiega sul monitoraggio attraverso il motore;
- i nomi di quei controlli sono un'interfaccia: vanno fissati a una versione di `fcp-server` e verificati a ogni suo aggiornamento;
- il livello comandi espone il mixer della scheda come comandi (`device.mixer.*`), così cronologia, superfici di controllo e MCP lo trattano come il resto;
- R-01 resta un requisito del percorso attraverso il motore (necessario per ascoltare effetti e strumenti virtuali); il monitoraggio diretto lo affianca, non lo sostituisce;
- il pacchetto Buildroot di `fcp-server` serve dalla Fase 1 se vogliamo impostare la scheda da dawOS, altrimenti dalla Fase 5.

### 2.7 Struttura del piano di esecuzione — aggiunte

Il testo della sezione 2.7 resta valido. Si aggiungono un campo, due tabelle e la sottosezione seguente.

**Campi e tabelle nuovi.**

```rust
#[repr(C)]
pub struct SegmentDesc {
    pub region: u32,       // shared memory region of the segment
    pub core: u8,          // live or anticipative core the host thread is pinned to
    pub priority: u8,      // SCHED_FIFO priority of the host thread
    pub serial_index: u16, // position among the segments of the same core
    pub cost_ns: u32,      // estimated cost of the plugin chain per cycle
    pub hop_ns: u32,       // cost of the cross-process hand-off, per cycle
    pub plugins: Span,     // plugin instances processed by this segment
}

#[repr(C)]
pub struct CoreBudget {
    pub core: u8,
    pub domain: u8,
    pub _pad: u16,
    pub serial_ns: u32,    // sum of (cost_ns + hop_ns) of the segments on this core
    pub floating_ns: u32,  // share of non-pinned work attributed by the simulation
    pub finish_ns: u32,    // simulated completion time of the last task on this core
    pub slack_ns: u32,     // deadline budget minus finish_ns
}
```

La tabella dei segmenti usa `SegmentDesc`; una nuova tabella dei budget contiene un `CoreBudget` per core. Quest'ultima non serve al motore per eseguire: serve a `dawos-health` e alla shell per mostrare dove sta il margine.

#### Verifica del budget

I segmenti esterni assegnati allo stesso core **non si eseguono in parallelo**: il worker di quel core li chiama uno dopo l'altro, e ogni chiamata comprende il salto verso il processo host e il ritorno. Il compilatore del piano deve quindi ragionare per core, non solo sul totale.

Definizioni, per un ciclo di durata `Q`:

- `cost(s)`: costo stimato del segmento `s`, cioè la somma dei costi dei suoi plugin per il numero di campioni del ciclo. Il costo di un plugin è il 99-esimo percentile misurato, se esiste; altrimenti il valore dichiarato dal manifest o rilevato alla scansione.
- `hop(s)`: costo del passaggio tra processi, una costante misurata per macchina; valore iniziale 10 µs _(stima)_.
- `D`: tempo utile, cioè `Q` meno una guardia per il ritardo del risveglio e la grana dei pacchetti USB; valore iniziale della guardia 0,4 ms _(stima)_, da misurare nella Fase 1.
- `U`: utilizzo massimo ammesso, 0,60 (ADR 0011).

Condizioni, tutte necessarie, verificate separatamente per il dominio live e per quello anticipativo:

1. **Carico seriale per core.** Per ogni core `c`: la somma di `cost(s) + hop(s)` sui segmenti assegnati a `c` non supera `U × Q`.
2. **Carico del dominio.** La somma di tutti i costi, nativi ed esterni, non supera `U × Q ×` numero di core del dominio.
3. **Tempo di completamento.** Il compilatore **simula la schedulazione** del ciclo: percorre le attività in ordine topologico; ogni segmento esterno parte quando sono pronti i suoi ingressi **e** il suo core è libero; ogni attività nativa parte sul primo worker libero. Il tempo di fine dell'ultima attività non supera `U × D`.

La terza condizione è quella che cattura la serializzazione: due tracce pesanti sullo stesso core, o un bus che deve aspettare tracce elaborate su un altro core, allungano il completamento anche quando il carico totale è basso.

Assegnazione dei segmenti ai core: riempimento per costo decrescente sul core meno carico, deterministico. L'assegnazione è **stabile** tra un piano e il successivo: un segmento cambia core solo se serve a rientrare nelle condizioni, perché spostarlo costa una chiamata di affinità e una cache fredda.

La verifica gira prima di ogni operazione che aumenta il carico: attivazione di un modulo, inserimento di un plugin, promozione al live, cambio di frequenza di campionamento. Se fallisce, il comando è rifiutato con `E_BUDGET`, e i dettagli dell'errore dicono quale condizione è violata, su quale core, di quanto, e quali tracce converrebbe congelare.

La simulazione usa costi stimati: non è una garanzia. Il watchdog e la scadenza per nodo restano l'ultima difesa.

### 3.2 Struttura del repository

Com'è dopo il passo P0.2. Le voci segnate «più avanti» non esistono ancora.

```text
daw-os/
├── README.md
├── LICENSE                       GPLv3 più eccezione per i plugin
├── VERSION                       versione dell'immagine, nella forma 1.2.3
├── Makefile                      punti d'ingresso: check-env, image, qemu, qemu-test, lint
├── .gitignore  .gitattributes  .gitmodules
├── .markdownlint.jsonc  .markdownlint-cli2.jsonc
├── .github/workflows/lint.yml    shellcheck e markdownlint
├── docs/
│   ├── 00-revisione-specifiche-fase0.md
│   ├── 01-registro-modifiche-v1.md           questo documento
│   ├── adr/                      una decisione per file
│   ├── fasi/fase-0/              un file per passo: consegna, comandi, attese
│   └── misure/fase-0/            rapporti e dati grezzi (dal passo P0.4)
├── buildroot/                    sottomodulo git, tag 2026.02.3
├── external/                     albero BR2_EXTERNAL, nome DAWOS
│   ├── external.desc  external.mk  Config.in
│   ├── configs/dawos_x86_64_dev_defconfig
│   ├── board/dawos/x86_64/
│   │   ├── linux/                frammenti del kernel: base, storage, qemu
│   │   │                         più avanti: rt, hw-285k, audio, debug
│   │   ├── cmdline/              common.txt, profile-generic.txt
│   │   │                         più avanti: profile-285k.txt
│   │   ├── genimage.cfg.in       modello dell'immagine disco per QEMU
│   │   ├── users.table           utenti e gruppi aggiunti (gruppo clock)
│   │   ├── rootfs-overlay/       unità systemd, rete, risolutore, rapporto di avvio
│   │   ├── post-build.sh         identità dell'immagine in os-release
│   │   └── post-image.sh         voce di avvio e immagine disco
│   ├── package/                  pacchetti nostri (vuoto per ora)
│   └── patches/linux/linux.hash  impronta dell'archivio del kernel
├── tools/
│   ├── check-env.sh              verifica dell'ambiente (accettazione di P0.1)
│   └── run-qemu.sh               avvio in QEMU, interattivo o di prova
├── crates/                       più avanti: spazio di lavoro Rust, dalla Fase 1
├── output/                       ignorato da git: albero di costruzione
├── dl/                           ignorato da git: archivio degli scaricamenti
├── keys/                         ignorato da git: chiavi di sviluppo (dal passo P0.5)
└── local/                        ignorato da git: dati di questa macchina
```

Differenze rispetto al documento 0:

- **genimage al posto di `systemd-repart`** per comporre l'immagine disco. genimage è costruito da Buildroot tra gli strumenti host, quindi il risultato non dipende dalla versione di systemd della distribuzione in WSL2. `systemd-repart` resta lo strumento per partizionare l'NVMe reale, dove gira dentro dawOS (passo P0.4).
- **File `VERSION`** alla radice: unica fonte della versione, letta da entrambi gli script.
- **Cartella `local/`** ignorata da git per i dati specifici della macchina: identità del disco di destinazione, chiavi pubbliche SSH autorizzate, indirizzo del server di aggiornamento.
- I documenti dei passi stanno in `docs/fasi/fase-0/`, uno per passo.

### 3.3 Schema del disco

GPT sull'NVMe da 1 TB del 285K.

| N. | Etichetta | Dimensione | Contenuto |
| --- | --- | --- | --- |
| 1 | `esp` | 1 GiB | systemd-boot e le UKI, due o tre versioni. |
| 2 | `dawos-root-<versione>` | 4 GiB | Slot A: EROFS. |
| 3 | `dawos-verity-<versione>` | 128 MiB | Slot A: albero di hash. |
| 4 | `_empty` → slot B | 4 GiB | Slot B, scritto dal primo aggiornamento. |
| 5 | `_empty` → slot B | 128 MiB | Slot B: albero di hash. |
| 6 | `state` | 32 GiB | `/var`. Nella Fase 0 ext4 in chiaro; LUKS2 con TPM dalla Fase 6. |
| 7 | `shared` | 300 GiB | XFS con reflink: `/data/shared`. |
| 8 | `homes` | il resto, circa 650 GiB | XFS che ospiterà le immagini di homed. |

L'immagine disco per QEMU (`output/images/disk.img`) contiene solo le partizioni 1 e 2, con la ESP ridotta a 256 MiB; le altre arrivano nel passo P0.5. L'NVMe reale non si scrive copiando quell'immagine: lo partiziona l'installatore, con le dimensioni di questa tabella.

### P0.1 — Ambiente di sviluppo

Riscritto per due macchine. Testo completo, file e comandi in [P0.1-ambiente.md](fasi/fase-0/P0.1-ambiente.md).

In sintesi: WSL2 sul PC di sviluppo, dimensionato sulle sue risorse; repository nel file system di Linux; `PATH` senza i percorsi di Windows; virtualizzazione annidata per `/dev/kvm`. Il criterio di accettazione è un comando: `make check-env` deve terminare con `RESULT: PASS`.

### P0.2 — Scheletro e primo avvio in QEMU

Eseguito. Testo completo, file e comandi in [P0.2-scheletro-e-qemu.md](fasi/fase-0/P0.2-scheletro-e-qemu.md).

Differenze rispetto al documento 0: immagine composta con genimage; radice indicata per etichetta GPT con la versione fin da ora; rapporto di avvio sulla console e prova automatica `make qemu-test`; impronta obbligatoria per l'archivio del kernel; LLMNR e mDNS spenti, perché systemd-resolved li apre verso la rete per impostazione predefinita.

La costruzione completa e l'avvio in QEMU sono stati provati (29 minuti su 4 core; `make qemu-test` → `PASS`).

### P0.4 — Primo avvio sul 285K e rumore di fondo del firmware

Questo passo viene prima del lavoro su verity e firma, perché affronta il rischio K-01. Con due macchine cambia il modo in cui l'immagine arriva sul disco.

**Portare l'immagine sul 285K.**

1. Sul PC di sviluppo, `make image` produce l'immagine `dev`; la si scrive su una **chiavetta USB** da Windows con uno strumento di copia grezza. WSL2 non vede in modo affidabile le chiavette USB, quindi la scrittura si fa dal lato Windows _(da verificare; l'alternativa è `usbipd`)_.
2. Il 285K si avvia dalla chiavetta: è un dawOS `dev` completo, con SSH.
3. Dal dawOS avviato dalla chiavetta si lancia l'installatore, `dawos-install`, che partiziona l'NVMe secondo la sezione 3.3, copia ESP e slot A, e crea `state`, `shared` e `homes`. Il lavoro vero lo fa `systemd-repart`, con le definizioni in `repart.d/` dentro l'immagine; lo script è l'involucro di sicurezza.
4. Si toglie la chiavetta e si riavvia dall'NVMe. Secure Boot è ancora disattivato.

**Regole di sicurezza dello script di scrittura** (requisito della revisione):

- il disco di destinazione si indica **per modello e numero di serie**, mai per percorso (`/dev/nvme0n1` può cambiare tra un avvio e l'altro). I due valori stanno in `local/target-disk.conf` sul PC di sviluppo, ignorato da git, e vengono inclusi nell'immagine: l'installatore conosce un solo disco e non ne accetta altri;
- deve esistere **esattamente un** disco con quel modello e quel numero di serie, altrimenti lo script si ferma;
- lo script **rifiuta** il disco se: è rimovibile o collegato via USB; ospita la radice in esecuzione, la ESP da cui si è avviato o un qualunque file system montato; ha partizioni in uso (swap, dispositivi mappati); non è un NVMe; è più piccolo di 500 GB;
- prima di scrivere **mostra**: modello, numero di serie, dimensione, la tabella delle partizioni attuale con etichette e file system, e lo schema che sta per creare;
- la conferma è **digitata**: bisogna scrivere per intero il numero di serie del disco. Un «sì» non basta;
- senza l'opzione `--write` lo script fa solo la prova a secco e non tocca nulla;
- tutto ciò che fa viene scritto sulla console e in un registro.

Lo script è una consegna di questo passo, non del passo P0.2.

**Rilevare la topologia e archiviarla**: `lscpu -e`, `/sys/devices/cpu_core/cpus`, `/sys/devices/cpu_atom/cpus`, condivisione della L2 da `/sys/devices/system/cpu/cpu*/cache/index2/shared_cpu_list`, `lspci -nn`, `lsusb -t`, `/proc/interrupts`. I risultati vanno in `docs/misure/fase-0/topologia.md` e decidono il frammento `hw-285k.fragment`.

**Impostare il firmware.** Lista di partenza: grafica primaria integrata; emulazione USB legacy disattivata; CSM disattivato; gestione del risparmio energetico lasciata al sistema operativo; ASPM disattivato; illuminazione e controller accessori disattivati; funzioni di regolazione dinamica del produttore disattivate; profilo di memoria stabile. Ogni voce cambiata si annota.

**Misurare il rumore hardware** su tutti i core, senza isolamento, per almeno 4 ore, con `rtla hwnoise` e con il contatore di SMI. Poiché `turbostat` non è tra gli strumenti offerti da Buildroot, il passo comprende un pacchetto nostro in `external/package/` che lo costruisce dai sorgenti del kernel.

**Misurare consumo e rumore** con la RTX 5080 nelle tre condizioni di K-14.

**Accettazione**: il 285K si avvia dall'NVMe; `make check-env` non è coinvolto; rumore hardware massimo ≤ 15 µs su ogni core; contatore di SMI fermo per tutta la prova; topologia archiviata. Se il criterio sul rumore non passa, ci si ferma e si decide insieme.

### P0.5 — Catena di avvio — aggiunta: aggiornamento dal PC di sviluppo

Il testo del passo P0.5 resta valido, con genimage al posto di `systemd-repart` nel punto 4. Si aggiunge il percorso di aggiornamento, così il meccanismo A/B viene collaudato a ogni iterazione dello sviluppo e non solo una volta.

**Sul PC di sviluppo.**

- `make image` produce, accanto all'immagine disco, una cartella di rilascio con: la UKI firmata, l'immagine della radice, l'albero di hash, il file `SHA256SUMS` e la sua firma.
- La firma del file delle impronte usa una **chiave di sviluppo** generata in `keys/`, mai nel repository; la parte pubblica entra nell'immagine.
- `make serve` espone la cartella con un server HTTP. L'integrità non dipende dal trasporto, perché le impronte sono firmate: HTTP in chiaro sulla rete locale è accettabile per `dev`.
- WSL2 deve essere raggiungibile dalla rete locale: rete in modalità speculare (`networkingMode=mirrored`) più una regola del firewall di Windows per la porta scelta _(da verificare sulla versione di Windows del PC di sviluppo)_.

**Sul 285K.**

- Tre definizioni di trasferimento in `/usr/lib/sysupdate.d/`: radice nello slot libero, albero di hash nello slot libero, UKI nella ESP con il contatore dei tentativi. `systemd-sysupdate` è disponibile nel tag (`BR2_PACKAGE_SYSTEMD_SYSUPDATE` _(verificato)_).
- L'indirizzo del server viene da `local/` al momento della costruzione.
- Un aggiornamento è: `systemd-sysupdate update`, riavvio, avvio dal nuovo slot con tre tentativi, conferma con `systemd-bless-boot`.
- La versione di un'immagine `dev` deve crescere a ogni costruzione, altrimenti `systemd-sysupdate` non la considera nuova: alla versione si aggiunge un suffisso con data e ora. Lo schema esatto si fissa in questo passo.

**Accettazione aggiuntiva**:

- un aggiornamento dal PC di sviluppo porta il 285K alla nuova versione senza toccare la chiavetta;
- un'immagine con impronta alterata sul server viene rifiutata prima della scrittura;
- un'immagine che non completa l'avvio fa tornare il sistema allo slot precedente dopo tre tentativi, senza intervento.

### P0.6 — Partizione dei core — esperimenti aggiunti

L'elenco degli esperimenti del passo P0.6 diventa:

| Id | Esperimento | Decide |
| --- | --- | --- |
| E-01 | Polling contro C1 e C1E sui core live | [ADR 0009](adr/0009-riposo-dei-core-live.md) |
| E-02 | _Fair server_ predefinito contro disattivato | K-04 |
| E-03 | Partizione isolata su cgroup _threaded_; affinità attraverso il confine | [ADR 0002](adr/0002-isolamento-statico-dei-core.md) |
| E-04 | `i915` contro `xe` come sorgente di rumore | Driver grafico |
| E-05 | Streaming dentro un'immagine LUKS su loop, su `homes` | [ADR 0004](adr/0004-partizioni-dei-dati.md) |
| E-06 | Rapporto di prestazioni tra E-core e P-core su carico DSP | [ADR 0011](adr/0011-r04-carico-sintetico.md) |

#### E-05 — Streaming dentro le home

**Scopo**: sapere se un progetto dentro una home di homed (XFS in un'immagine LUKS2 su dispositivo loop, ospitata su `homes`) regge lo streaming di R-04, o se serve il ripiego.

**Configurazioni a confronto**, tutte sullo stesso NVMe:

| Sigla | Pila | Rappresenta |
| --- | --- | --- |
| S | XFS su `shared`, in chiaro | Il riferimento richiesto dalla revisione. |
| P | XFS su LUKS2 direttamente sulla partizione `homes` | Il ripiego (partizione LUKS per utente) e, insieme, il costo della sola cifratura. |
| L | XFS su LUKS2 su dispositivo loop in I/O diretto, file immagine su XFS in `homes` | La scelta attuale. |

La configurazione P si prova per prima, perché distrugge il contenuto di `homes`, che nella Fase 0 è vuota; poi si ricrea XFS e si prova L. Per P e L si provano anche le opzioni di dm-crypt che evitano le code di lavoro in lettura e in scrittura.

**Carichi**, con `fio`, motore `io_uring`, `O_DIRECT`:

| Sigla | Carico | Modella |
| --- | --- | --- |
| W1 | Lettura sequenziale, blocchi da 1 MiB, profondità 32, 60 secondi | Il riempimento dei buffer dopo uno spostamento del cursore. |
| W2 | 16 flussi di lettura da 256 KiB limitati a 400 MB/s in totale, più 16 scritture sequenziali su file preallocati a 6 MB/s in totale, 10 minuti | La variante stress a 192 kHz in riproduzione, con 16 tracce in registrazione. |
| W3 | Aggiunte da 4 KiB con `fdatasync` a ogni scrittura, 2 minuti | Il journal del progetto. |

I 400 MB/s di W2 vengono da 256 tracce stereo a 192 kHz in virgola mobile a 32 bit: 256 × 192 000 × 2 × 4 byte = 393 MB/s.

**Soglie, fissate ora.** Il ripiego diventa necessario se la configurazione L viola anche una sola di queste condizioni e la configurazione P le rispetta:

| Grandezza | Soglia relativa a P | Soglia assoluta |
| --- | --- | --- |
| W1, portata | almeno il 70 % | almeno 1,2 GB/s |
| W2, latenza di completamento al 99,9-esimo percentile | al più il doppio | al più 20 ms |
| W2, latenza massima | — | al più 200 ms |
| W2, utilizzo del dominio di I/O (cluster E4) | — | al più il 50 % |
| W3, `fdatasync` al 99-esimo percentile | al più il doppio | al più 10 ms |
| Latenza massima di `timerlat` sui core live con W2 in corso | peggioramento al più di 5 µs | entro la soglia di fase |

La soglia di 1,2 GB/s è tre volte la portata della variante stress: riempire un secondo di audio di tutte le tracce dopo uno spostamento deve richiedere meno di mezzo secondo.

Se anche P viola le soglie, il problema è la cifratura in sé e non il dispositivo loop: in quel caso non si adotta il ripiego ma si riapre la discussione sulla cifratura degli spazi utente.

Queste soglie sono una mia proposta ragionata _(stima)_, **confermata** nella revisione del 13 novembre 2026.

#### E-06 — Rapporto tra E-core e P-core

**Scopo**: misurare il numero da cui dipende l'esito di R-04 sul dominio anticipativo.

**Metodo**: un piccolo programma di misura esegue una cascata di filtri del secondo ordine su blocchi stereo, in AVX2, fissato prima su un P-core e poi su un E-core alle frequenze di lavoro, con blocchi da 256 e da 2048 campioni. Riporta i nanosecondi per campione e il rapporto.

**Esito**: il rapporto entra nell'ADR 0011 al posto della forbice 0,55–0,80 e dice se la sessione di riferimento a 96 kHz è dentro o fuori dal 60 %.

Questo esperimento richiede un programma di misura di poche decine di righe: uno strumento, non codice di prodotto. **Approvato** nella revisione del 13 novembre 2026.

### 3.5 Strumenti di misura — correzioni

- `turbostat` **non** è tra gli strumenti del kernel di Buildroot 2026.02.3 _(verificato)_: serve un pacchetto nostro, che lo costruisce da `tools/power/x86/turbostat` dei sorgenti del kernel. Arriva con il passo P0.4.
- `rtla` è disponibile (`BR2_PACKAGE_LINUX_TOOLS_RTLA`) _(verificato)_.
- `ukify` è costruito solo tra gli strumenti host _(verificato)_; il modo di richiamarlo da `post-image.sh` si verifica nel passo P0.5.

### Parte 4 — Stato delle domande

| Domanda | Stato | Dove |
| --- | --- | --- |
| D-01 | Chiusa: due macchine | [ADR 0001](adr/0001-due-macchine.md) |
| D-02 | Chiusa: isolamento statico al boot | [ADR 0002](adr/0002-isolamento-statico-dei-core.md) |
| D-03 | Chiusa: nessun initrd, con prova a tempo | [ADR 0003](adr/0003-avvio-senza-initrd.md) |
| D-04 | Chiusa: tre partizioni, `shared` da 300 GiB | [ADR 0004](adr/0004-partizioni-dei-dati.md) |
| D-05 | Chiusa: varianti `dev` e `prod` | [ADR 0005](adr/0005-varianti-dev-e-prod.md) |
| D-06 | Chiusa: toolchain Bootlin con glibc | [ADR 0006](adr/0006-toolchain-bootlin-glibc.md) |
| D-07 | Chiusa: Buildroot 2026.02.x e kernel fissato | [ADR 0007](adr/0007-versioni-buildroot-e-kernel.md) |
| D-08 | Chiusa: `fcp-server` come processo separato | [ADR 0008](adr/0008-fcp-server-processo-separato.md) |
| D-09 | In sospeso fino al passo P0.6 | [ADR 0009](adr/0009-riposo-dei-core-live.md) |
| D-16 | Chiusa: carico sintetico, soglia del 60 %, freeze nell'MVP | [ADR 0011](adr/0011-r04-carico-sintetico.md) |
| D-10…D-15, D-17…D-21 | Rinviate: le ripropongo prima della Fase 1 con le raccomandazioni aggiornate | — |

Due effetti delle decisioni prese sulle domande rinviate, da tenere a mente:

- **D-15**: il freeze è già deciso; restano automazione, mappa del tempo, compensazione della latenza e le altre voci di M-02.
- **D-14**: con il monitoraggio diretto della Scarlett (M-12), un eventuale superamento di R-01 a 44,1 kHz pesa meno per chi registra, ma resta intero per chi suona strumenti virtuali.

## 3. Punti aperti introdotti da questa revisione

Tutti e tre **chiusi il 13 novembre 2026**, come proposto.

| Punto | Decisione |
| --- | --- |
| Freeze nella sessione di riferimento | A 96 kHz senza tracce congelate; variante stress a 192 kHz con il freeze ammesso, dichiarando quante tracce è stato necessario congelare. |
| Soglie dell'esperimento E-05 | Confermate. |
| Esperimento E-06 | Approvato: entra nel passo P0.6. |

## 4. Il disco del 285K

Sul 285K c'è oggi un solo disco da 932 GB con Windows 11. **Deciso il 13 novembre 2026**: è il disco dedicato a dawOS, e il passo P0.4 lo cancella per intero. Prima va salvato ciò che si vuole tenere. I dettagli e le conseguenze per lo script di scrittura sono in [hardware-285k.md](fasi/fase-0/hardware-285k.md).

## 5. Correzioni emerse dal passo P0.3

Due affermazioni del documento 0 corrette dalla prova sul kernel compilato.

| Sezione | Era scritto | È vero |
| --- | --- | --- |
| P0.3, accettazione | «`cat /sys/kernel/realtime` dà `1`» | Quel file non esiste nel kernel 6.18; apparteneva alle patch real-time esterne. Il kernel real-time si riconosce da `uname -v`, che contiene `PREEMPT_RT`. |
| P0.3, tabella delle opzioni | «pagine grandi trasparenti a `madvise`» | Con `PREEMPT_RT` le pagine grandi trasparenti non sono disponibili affatto (`mm/Kconfig`). Le pagine da 2 MB per il preload dei sample e per la memoria condivisa vanno riservate con `hugetlbfs`. Il parametro `transparent_hugepage=madvise` previsto nella riga di comando del passo P0.6 è quindi inutile e va tolto. |

Il dettaglio è in [P0.3-kernel-real-time.md](fasi/fase-0/P0.3-kernel-real-time.md).

## 6. Modifiche emerse dal passo P0.4a

| Sezione | Era scritto | Com'è ora |
| --- | --- | --- |
| P0.4 | Un solo passo | Diviso in **P0.4a** (installazione e accesso remoto) e **P0.4b** (topologia, firmware, rumore di fondo). |
| P0.4, identità del disco | Modello e numero di serie inclusi nell'immagine da `local/target-disk.conf` | Indicati sul 285K con `--model` e `--serial`, dopo `dawos-install --list`. Approvato il 13 novembre 2026. |
| 3.3, partizione `homes` | Circa 650 GiB | 590,7 GiB, misurati nella prova generale su un disco da 932 GiB. |
| 3.3, immagine per QEMU | Una sola immagine | Due: `disk.img` e `installer.img`, con etichette di radice diverse. |
| ADR 0005, password di root | Bloccata dal passo P0.4 | Resta vuota per la sola console; via SSH le password sono disattivate. Approvato il 13 novembre 2026. |
| ADR 0005, chiave dell'host SSH | Non specificata | Generata sul PC di sviluppo in `keys/` e inclusa nelle immagini `dev`. Approvato il 13 novembre 2026. |
| P0.4, rete | DHCP | Indirizzo statico da `local/network-285k.conf`, legato al driver della scheda. |

Il dettaglio è in [P0.4a-installazione.md](fasi/fase-0/P0.4a-installazione.md).

## 7. Modifiche emerse dalla misura di partenza di P0.4b

| Sezione | Era scritto | Com'è ora |
| --- | --- | --- |
| Rischi | K-01 … K-14 | Nuovo rischio **K-15 — Dissipazione termica del 285K**: nella misura di partenza il package è arrivato a 101 °C con 120–160 W. Gravità alta finché non è risolto; riguarda le fasi 0 (P0.4b, P0.6, P0.7) e 3 (R-04). Mitigazioni in discussione: limite di potenza fisso nel firmware, raffreddamento. |
| P0.4, impostazioni del firmware | Dieci voci da cambiare prima della seconda misura | Proposta di ridurle a quelle con una ragione indipendente dagli SMI, visto che la misura di partenza ne ha contati zero. In attesa di conferma. |
| P0.4, criterio | Rumore ≤ 15 µs e zero SMI | Invariato; in più il riepilogo segnala a parte la temperatura da 95 °C e gli interventi della protezione termica. |
| 3.5, `rtla hwnoise` | Soglia e copertura non specificate | Risoluzione di 1 µs e copertura del 75 % del tempo per core, verificate nei sorgenti della 6.18.63. |

Il dettaglio è in [P0.4b-rumore-di-fondo.md](fasi/fase-0/P0.4b-rumore-di-fondo.md).
