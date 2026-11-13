# dawOS — Documento 0: revisione critica, interfacce trasversali, piano della Fase 0, domande aperte

- **Stato**: bozza in attesa di revisione. Nessun codice di prodotto è stato scritto.
- **Data**: 13 novembre 2026.
- **Ambito**: risposta al «Primo compito» del master prompt.

## Come leggere questo documento

Il documento ha quattro parti, nell'ordine chiesto. Chi ha poco tempo legge la sintesi qui sotto e la Parte 4: le domande D-01…D-09 bloccano l'inizio della Fase 0.

Ogni affermazione tecnica non ovvia porta una di queste etichette.

| Etichetta | Significato |
| --- | --- |
| _(verificato)_ | Controllato in questa sessione su una fonte elencata in fondo. |
| _(da verificare)_ | Lo ritengo vero per conoscenza pregressa, ma va confermato sulla versione reale prima di dipenderne. |
| _(stima)_ | Numero ragionato, non misurato. Serve a dimensionare, non a promettere. |

Identificativi usati: `C-nn` contraddizioni, `K-nn` rischi (la lettera R è già dei requisiti), `M-nn` parti mancanti, `D-nn` domande aperte, `P0.n` passi della Fase 0.

### Versioni su cui mi baso

| Componente | Versione di riferimento | Nota |
| --- | --- | --- |
| Kernel | Linux 6.18.y LTS, fine supporto prevista dicembre 2027 _(verificato)_ | Oggi la serie stabile è la 7.x; la LTS del 2026 non è ancora designata. `PREEMPT_RT` è nel ramo principale dalla 6.12, quindi niente patch esterne _(da verificare: eventuali residui della coda RT per 6.18)_. |
| Buildroot | 2026.02.x LTS _(verificato che esista; uscita 4 marzo 2026)_ | Esistono anche 2026.05 e 2026.08 (supporto di tre mesi). Versione del kernel predefinita e opzioni per UKI da verificare sul tag reale. |
| systemd | ≥ 257 | Le UKI a più profili e `ukify --join-profile` sono documentate nelle pagine 257–262 _(verificato)_; versione esatta d'introduzione _(da verificare)_. |
| VST3 SDK | 3.8.0 (ottobre 2025), licenza MIT _(verificato)_ | Include modifiche alle interfacce per MIDI 2.0. |
| Driver FCP | `sound/usb/fcp.c`, presente nei sorgenti 6.17 e 6.18 _(verificato)_ | L'apertura del dispositivo hwdep richiede `CAP_SYS_RAWIO` _(verificato)_. USB ID della 18i20 di quarta generazione: `1235:821d` _(verificato)_. |
| Smithay, wgpu, egui | Non fissate | Si fissano all'inizio della Fase 3; cambiano troppo in fretta per sceglierle ora. |

## Sintesi

L'architettura è solida e coerente nelle linee principali: motore con piano compilato, separazione live e anticipativo, livello comandi unico, radice immutabile con A/B. Non ho trovato decisioni da rovesciare. Ho trovato però sei punti in cui due decisioni non possono valere insieme così come sono scritte, e tre rischi che possono far fallire un criterio di accettazione indipendentemente dalla qualità del codice.

I cinque punti su cui ti chiedo attenzione, in ordine:

1. **I thread di un processo non possono stare in due partizioni cgroup** (C-02). Motore e host dei plugin hanno thread live e thread anticipativi nello stesso processo; le partizioni isolate di cgroup v2 e i cgroup per processo (memoria, nftables, sandbox) tirano in direzioni opposte. Propongo isolamento statico al boot per l'MVP.
2. **La regola «nessuna chiamata di sistema» non regge con un host per traccia in un altro processo** (C-01). Serve un'eccezione precisa (`futex`) e un modello di passaggio tra processi; ne propongo uno sincrono sullo stesso core.
3. **La latenza del firmware (SMI) è fuori dal nostro controllo** (K-01) e va misurata per prima, prima di investire nel resto della Fase 0.
4. **R-04 non è misurabile finché non definiamo quanto pesa un «plugin medio»** (K-02): con plugin leggeri la sessione di riferimento è plausibile, con plugin commerciali medi no.
5. **XFS, systemd-homed e LUKS a due livelli vanno riconciliati ora** (C-04), perché decidono la tabella delle partizioni che creiamo nella Fase 0.

---

## Parte 1 — Revisione critica dell'architettura

### 1.1 Contraddizioni tra decisioni

#### C-01 — «Nessuna chiamata di sistema nei thread live» contro host per traccia, futex e attesa attiva

**Cosa non torna.** Tre decisioni insieme: i thread live non fanno chiamate di sistema tranne l'attesa su ALSA; i risvegli tra processi usano futex; il parallelismo live usa attesa attiva. Una traccia live con plugin esterni attraversa però un confine di processo (`dawos-engine` → `dawos-plughost` → `dawos-engine`) a ogni quanto. Per far lavorare il thread live dell'host ci sono solo due modi:

- l'host dorme e il motore lo sveglia con `futex_wake`, che è una chiamata di sistema nel thread live;
- l'host resta in attesa attiva su un atomico condiviso, ma con 16 tracce live più i bus ci sono 20–30 thread in attesa attiva su 4 core: con `SCHED_FIFO` a pari priorità un thread che gira a vuoto non cede mai il core e il sistema si blocca.

**Perché conta.** È il cuore del percorso live con plugin, cioè del requisito R-04. Se la regola resta ambigua, ogni componente la interpreterà a modo suo.

**Costo reale.** Il costo dei cambi di contesto è piccolo: due cambi per salto, 5–10 µs per salto compresi gli effetti di cache _(stima)_; con circa 24 salti per quanto sono 120–240 µs di CPU su 10,7 ms·core disponibili, cioè l'1–2 %. Il problema è il modello di scheduling, non il costo.

**Proposta.**

1. Riscrivere la regola così: _nei thread live sono vietate le chiamate di sistema bloccanti o di durata non limitata; sono ammesse solo l'attesa su ALSA, `futex_wake` e `futex_wait` sul passaggio tra processi e sul confine del ciclo._
2. Adottare un **passaggio sincrono sullo stesso core**: il compilatore del piano assegna ogni segmento esterno a un core live; il thread live dell'host è fissato su quel core con priorità 81, il worker del motore ha priorità 80. Il worker fa `futex_wake`, viene prelazionato subito dall'host, e riprende quando l'host torna in `futex_wait`. Buffer caldi in cache, nessuna attesa attiva tra processi, scadenza controllata da un watchdog esterno. I dettagli sono nella sezione 2.3.
3. Unità di IPC: il **segmento di catena** (tutti i plugin esterni consecutivi di una traccia in un solo salto), non il singolo plugin. Un effetto nativo in mezzo a due VST3 spezzerebbe il segmento in due salti: propongo che i moduli DSP nativi siano una libreria collegabile anche dentro l'host, così la catena resta un salto solo.

#### C-02 — Partizioni isolate di cgroup v2 contro cgroup per processo e thread in due domini

**Cosa non torna.** La decisione sul kernel prevede partizioni isolate di cgroup v2 ridimensionabili. Altre decisioni prevedono: limiti di memoria per processo, un'unità systemd con sandbox per ogni servizio, regole nftables per cgroup, e processi (`dawos-engine`, ogni `dawos-plughost`) che hanno insieme un thread live e thread anticipativi o di I/O.

I fatti del kernel:

- `sched_setaffinity` è limitata alle CPU effettive del cpuset del processo; le CPU cedute a una partizione isolata escono dal cpuset del resto dell'albero. Un thread fuori dalla partizione non può entrarci con la sola affinità.
- Un processo appartiene a un solo cgroup di dominio. Per mettere thread dello stesso processo in cgroup diversi serve un sottoalbero _threaded_, ma lì i controller non _threaded_ (memoria) si applicano solo alla radice del sottoalbero: tutti gli host finirebbero nello stesso cgroup di memoria, perdendo limiti per processo, scelta della vittima in caso di memoria esaurita e regole nftables per servizio.
- Se un cgroup _threaded_ possa essere radice di una partizione isolata (anche «remota», con `cpuset.cpus.exclusive`) non lo so con certezza _(da verificare con un esperimento nel passo P0.6)_.
- Le partizioni isolate nella 6.18 tolgono le CPU dal bilanciamento e dalle workqueue non vincolate (`HK_TYPE_DOMAIN`), ma `nohz_full` e `rcu_nocbs` restano parametri di boot; le serie che li rendono dinamici sono ancora proposte nel 2026 _(verificato)_.

**Perché conta.** Decide come ogni processo real-time viene avviato e confinato; cambiarlo dopo tocca `dawosd`, le unità systemd e la sicurezza.

**Proposta (richiede la tua decisione, vedi D-02).** Per l'MVP, dove la partizione è comunque statica: isolamento al boot con `isolcpus=domain,managed_irq` sui core live, più `nohz_full` e `rcu_nocbs`; ogni thread si colloca con `sched_setaffinity`; i cgroup restano per processo e servono solo a memoria, PID e rete. Le partizioni dinamiche di cgroup passano tra gli obiettivi di ricerca, insieme a `SCHED_DEADLINE`. Con un avvio sotto i 3 secondi, cambiare profilo di core riavviando è un costo accettabile: i profili diventano profili della UKI (vedi C-03).

Regole di contorno necessarie in ogni caso: il thread principale di ogni processo ha affinità sul dominio non live; solo il thread live si sposta da sé sui core live; `SCHED_RESET_ON_FORK` sempre attivo, così un thread creato da un plugin non eredita la priorità real-time; `dawos-health` segnala qualunque thread inatteso sui core live.

#### C-03 — «La numerazione dei core si legge da `/sys`» contro parametri di boot in una riga di comando firmata

**Cosa non torna.** `nohz_full`, `rcu_nocbs` e `isolcpus` si danno al boot, e la riga di comando è dentro la UKI firmata: non si può calcolarla a runtime leggendo `/sys`.

C'è un secondo vincolo: su x86 la CPU di boot (CPU 0) viene esclusa dal kernel dall'insieme `nohz_full` perché tiene il tempo di sistema _(da verificare sulla 6.18; il messaggio è «NO\_HZ: Clearing 0 from nohz\_full range for timekeeping»)_. Se sul 285K la CPU 0 è un P-core, come mi aspetto (P-core su CPU 0–7, E-core su 8–23 _(da verificare con `lscpu -e`)_), il dominio live non può contenere la CPU 0.

**Proposta.**

- I nomi P1…P8 diventano **etichette logiche**. La mappa fisica proposta è: live = CPU 4–7, `nohz_full` = CPU 2–7, anticipativo = CPU 0–3 più i cluster E1 ed E2, sistema = E3, I/O e interrupt = E4. La CPU 0 resta fuori dal live.
- La riga di comando specifica dell'hardware vive in un **profilo della UKI** (UKI a più profili: ogni profilo ha la sua riga di comando, misurata e firmata). Profilo `285k` con le maschere; profilo `generico` senza isolamento, usato in QEMU e come ripiego.
- All'avvio `dawosd` legge la topologia da `/sys`, la confronta con la riga di comando effettiva e, se non coincidono, avvia in modalità degradata con un avviso. La lettura da `/sys` resta quindi la fonte di verità per la verifica e per tutte le affinità a runtime.

#### C-04 — XFS con reflink, systemd-homed e cifratura a due livelli

**Cosa non torna.** Le decisioni chiedono: volume XFS con reflink che contiene `/data/shared` e `/home`; sistema in LUKS2 sbloccato dal TPM; spazi utente cifrati con systemd-homed.

- systemd-homed cifra in due modi: `fscrypt`, che XFS non supporta, oppure `luks`, cioè un file immagine LUKS2 con dentro un file system (btrfs, ext4 o XFS) _(verificato)_. Su XFS resta solo il secondo.
- Se le immagini delle home stanno su un volume a sua volta dentro il LUKS di sistema, lo streaming audio attraversa dm-crypt due volte più un dispositivo loop.
- XFS non si può rimpicciolire: la modalità di ridimensionamento automatico di homed è pensata per btrfs _(verificato)_.
- Il reflink funziona solo dentro lo stesso file system: tra `/data/shared` e la home di un utente non è possibile, e nemmeno tra utenti.

**Perché conta.** La tabella delle partizioni si crea nella Fase 0; spostare terabyte dopo è costoso.

**Proposta (vedi D-04).** Tre partizioni di dati invece di una:

| Partizione | Cifratura | Contenuto |
| --- | --- | --- |
| `state` | LUKS2 con TPM | `/var`: stato di sistema, registri, record di homed, rapporti di salute. |
| `shared` | LUKS2 con TPM | XFS con reflink: `/data/shared`. |
| `homes` | nessuna a questo livello | XFS che ospita solo le immagini LUKS2 di homed; dentro ogni immagine, XFS con reflink e quote di progetto (servono per la quota dell'agente). |

Ogni dato utente resta cifrato una volta sola. Le immagini delle home sono file sparsi con `discard`, così lo spazio non usato torna al volume esterno anche senza rimpicciolire XFS. Il reflink resta disponibile dove serve davvero: dentro la home, tra progetto e sue versioni. In alternativa si rinuncia a homed e `dawosd` gestisce un volume LUKS2 per utente; è più codice nostro e la sconsiglio.

#### C-05 — «Nessun servizio in ascolto» contro server MCP nell'MVP e accesso di sviluppo

**Cosa non torna.** Il server MCP è nell'MVP, l'agente no. Senza servizi in ascolto e senza agente locale, nessun client può raggiungerlo. Lo stesso vale per noi: senza SSH non possiamo lanciare `rtla` né leggere i risultati sul PC reale.

**Proposta (vedi D-05).** Due varianti della stessa immagine, costruite dallo stesso albero.

- `dev`: SSH con sola chiave, strumenti di misura e tracciamento, console seriale, MCP raggiungibile attraverso un tunnel SSH. Firmata solo con la chiave di sviluppo.
- `prod`: nessun servizio in ascolto, MCP solo su socket Unix locale. Arriva nella Fase 6.

Nella Fase 0 esiste solo `dev`. La regola «nessun servizio in ascolto» si applica a `prod`.

#### C-06 — «Avvio riuscito solo dopo audio senza xrun» contro avvii senza scheda e xrun dovuti al progetto

**Cosa non torna.** Se la Scarlett è spenta o scollegata l'avvio non potrebbe mai essere dichiarato riuscito. Se invece gli xrun dipendono da un progetto pesante o da un plugin difettoso, un aggiornamento sano verrebbe annullato.

**Proposta.** Il criterio di avvio riuscito è una **autoverifica con carico sintetico**, eseguita prima di aprire qualunque contenuto dell'utente: il motore esegue per 10 secondi un grafo di prova fisso. Con la scheda presente si richiedono zero xrun; senza scheda si usa un controllo di latenza di scheduling sui core live (stile `timerlat`, soglia 50 µs). Gli xrun successivi, a progetto aperto, alimentano `dawos-health` ma non il ritorno allo slot precedente. Nella Fase 0, dove il motore non esiste ancora, il criterio è semplicemente il raggiungimento del target di sistema.

### 1.2 Rischi tecnici in ordine di gravità

| Id | Rischio | Gravità | Quando si scopre |
| --- | --- | --- | --- |
| K-01 | Latenza hardware e firmware (SMI) sopra soglia | Critica | Fase 0, primo giorno sul PC reale |
| K-02 | R-04 non misurabile e forse irrealistico con plugin medi | Alta | Fase 4, troppo tardi se non si definisce ora |
| K-03 | R-01 con margine sottile a due periodi | Alta | Fase 1 |
| K-04 | Attesa attiva: thread del kernel affamati, calore e rumore | Alta | Fase 0 e Fase 1 |
| K-05 | Ampiezza del progetto rispetto alle forze | Alta | Continuo |
| K-06 | Plugin di terze parti non adatti al tempo reale | Alta | Fase 4 |
| K-07 | Interrupt xHCI condiviso con altri dispositivi USB | Media | Fase 1 |
| K-08 | Difetti audio che ALSA non segnala come xrun | Media | Fase 1 |
| K-09 | Ecosistema VST3 su Linux e librerie di runtime | Media | Fase 4 |
| K-10 | cage: editor flottanti e due monitor | Media | Fase 3 e 4 |
| K-11 | Avvio in meno di 3 secondi | Media | Fase 0 e Fase 6 |
| K-12 | Journal: crescita, versioni, riesecuzione | Media | Fase 2 |
| K-13 | Sblocco con solo TPM e vecchie UKI firmate | Media | Fase 6 |
| K-14 | RTX 5080 «spenta» | Bassa | Fase 0 |

#### K-01 — Latenza hardware e firmware

Su una scheda madre consumer gli SMI del firmware (gestione termica, emulazione USB, controller LED, EC) possono fermare tutti i core per decine o centinaia di microsecondi. Il kernel non può né impedirli né mascherarli. Se il rumore hardware supera da solo i 50 µs, il criterio della Fase 0 è irraggiungibile. Non ho trovato misure pubbliche di `PREEMPT_RT` sul 285K _(verificato che la ricerca non ne restituisce)_.

Proposta: misurare per prima cosa, appena un kernel RT minimo si avvia sul PC reale (passo P0.4), con `rtla hwnoise` e con il contatore di SMI letto da `turbostat`. Obiettivo: rumore hardware massimo sotto 15 µs e zero SMI durante la prova. Se non si raggiunge, si lavora sulle impostazioni del firmware (lista nel passo P0.4) prima di ogni altra cosa. Se nemmeno così, bisogna rivedere la soglia o la scheda madre, e conviene saperlo subito.

#### K-02 — R-04 non è misurabile senza definire il carico

Conti di dimensionamento _(stime)_ a 96 kHz, con quanto di 256 campioni (2,667 ms):

- **Live**: 4 core × 2,667 ms = 10,7 ms·core per quanto. Al 60 % di utilizzo restano 6,4 ms per 16 tracce × 8 plugin = 128 istanze più bus e master: circa 40 µs per istanza per quanto, cioè circa 156 ns per campione. Compatibile con plugin leggeri e medi.
- **Anticipativo**: 240 tracce × 8 = 1920 istanze su 4 P-core e 8 E-core. Stimando un E-core a 0,6 P-core si hanno circa 8,8 P-core equivalenti; al 70 % sono circa lo 0,33 % di un P-core per istanza, cioè circa 34 ns per campione. È il territorio di un equalizzatore leggero, non di un plugin commerciale medio (che a 96 kHz costa tipicamente 5–10 volte tanto).
- **Memoria**: 256 processi host con plugin commerciali possono occupare da 12 a 50 GB su 64, tutti bloccati in RAM.

Quindi la sessione di riferimento passa o fallisce a seconda di cosa si intende per «plugin». Proposta (vedi D-16): definire un **plugin di riferimento sintetico** con costo dichiarato e calibrato (per esempio tre classi: leggero, medio, pesante, in proporzioni fissate), e riformulare R-04 in termini di quel carico. La verifica del budget prima di attivare, già decisa, diventa allora un calcolo verificabile.

#### K-03 — R-01 con margine sottile

Il quanto dura 2,667 ms nelle frequenze della famiglia 48 kHz e 2,902 ms in quella 44,1 kHz. Con ingresso e uscita collegati e il periodo di uscita scritto nello stesso ciclo in cui si legge l'ingresso, la latenza del solo buffer è di due quanti: 5,33 ms (5,80 ms a 44,1 kHz). Restano 4,67 ms (4,20 ms) per conversione, FIFO interne della scheda e pacchetti USB, che su interfacce di questa classe valgono tipicamente 1,5–3,5 ms a 48 kHz _(stima; non ho trovato misure della 18i20 di quarta generazione su Linux)_.

Conseguenze:

- R-01 è raggiungibile solo nello schema sincrono a due periodi. Un terzo periodo di sicurezza, come spesso si consiglia per l'USB, porta il solo buffer a 8,0 ms (8,71 ms) e R-01 non regge più.
- Il tempo utile di calcolo è minore del quanto: i pacchetti USB arrivano raggruppati in URB, quindi il risveglio ha una grana propria. Va misurata.
- R-01 va misurato senza plugin con latenza sul percorso; con plugin latenti il monitoraggio si allunga (vedi M-01).

Proposta: nessun cambio di decisione; misura precoce nella Fase 1 e risposta anticipata alla domanda D-14 (se R-01 fallisce a 44,1 kHz, cosa è negoziabile: i 10 ms o i 128 campioni).

#### K-04 — Attesa attiva: thread del kernel affamati, calore e rumore

Due effetti distinti.

**Thread del kernel per CPU.** Anche su un core isolato restano thread del kernel legati a quella CPU. Dal 6.12 il vecchio limite ai processi real-time è sostituito dal _fair server_: se un thread `SCHED_FIFO` occupa il core senza mai dormire e un thread normale diventa eseguibile lì, il kernel lo fa girare prelazionando il thread real-time, fino a 50 ms al secondo per impostazione predefinita. Disattivarlo (i parametri sono in `/sys/kernel/debug/sched/fair_server/cpuN/` _(verificato)_) elimina la prelazione ma espone a blocchi di sistema se un lavoro per CPU non gira mai; le discussioni recenti sul kernel avvertono esplicitamente di questo.

Proposta: i worker live fanno attesa attiva **solo dentro il ciclo**; a fine ciclo dormono su un futex fino al ciclo successivo. Il core ha così una finestra regolare per i lavori del kernel, e il _fair server_ non ha motivo di intervenire. Costa un risveglio per worker per ciclo.

**Calore e rumore acustico.** Stato di polling permanente più attesa attiva significano 4–6 P-core sempre al massimo: decine di watt costanti e ventole che girano, in uno studio di registrazione. Proposta (vedi D-09): PM QoS attivo solo quando il motore è armato, e misura nella Fase 0 della differenza tra polling e C1 sui core live. Se C1 costa 1–2 µs di latenza, è probabilmente lo scambio giusto.

#### K-05 — Ampiezza del progetto

Sistema operativo, motore, sessione, host dei plugin, interfaccia, compositor, sicurezza, backup e agente sono, realisticamente, anni di lavoro. Il rischio non è tecnico ma di non arrivare mai a uno strumento usabile.

Proposta: fette verticali sottili e misurabili (la roadmap lo fa già); componenti sviluppabili e provabili anche fuori dall'immagine (backend audio nullo e su file, vedi M-07); nessun componente scritto prima della fase che lo richiede. Riuso di componenti esistenti come riferimento dove la licenza lo consente (per esempio `fcp-server` per il protocollo FCP).

#### K-06 — Plugin di terze parti non adatti al tempo reale

Un plugin può allocare, prendere lock o girare a vuoto dentro `process()`. Con `SCHED_FIFO` un thread che non si ferma tiene il core per sempre. La scadenza per nodo con silenzio e dissolvenza è già decisa; manca chi la fa rispettare.

Proposta: un thread watchdog nel motore, a priorità 95 su un core non live, con un timer per ciclo. Alla scadenza marca il segmento come abbandonato, declassa o termina l'host, e il worker riprende emettendo la dissolvenza. `RLIMIT_RTTIME` sarebbe una seconda difesa quasi gratuita, ma arma un timer di CPU che può riattivare il tick sui core `nohz_full` _(da verificare)_: per questo preferisco il watchdog esterno.

#### K-07 — Interrupt xHCI condiviso

La priorità 90 si applica al thread dell'interrupt del controller xHCI, non della Scarlett: lo stesso thread serve tastiere MIDI, mouse e dischi USB sullo stesso controller. Un disco di backup USB su quel controller mette traffico nella stessa coda.

Proposta: Scarlett da sola su un controller xHCI, identificato per indirizzo PCI; tutto il resto su un altro controller. La scheda madre ne ha di norma più d'uno _(da verificare con `lsusb -t` e `lspci`)_. `dawos-health` segnala la configurazione errata.

#### K-08 — Difetti audio che ALSA non segnala

Pacchetti isocroni persi o corti producono un clic senza che lo stato di ALSA passi a xrun. «Nessun xrun in 24 ore» può essere vero con audio difettoso.

Proposta: lo strumento di accettazione della Fase 1 verifica il **segnale**, non lo stato. In loopback si invia una sequenza nota e continua e si controlla ogni campione ricevuto; un difetto è qualunque discontinuità. Definizione completa in M-06.

#### K-09 — Ecosistema VST3 su Linux e librerie di runtime

I produttori che pubblicano VST3 nativi per Linux sono una minoranza (tra quelli che conosco: u-he, TAL, Tracktion, Modartt, Toneboosters, Auburn Sounds, più il software libero _(da verificare caso per caso)_). Uno «strumento primario» dipende dai plugin disponibili. Inoltre i binari sono compilati contro glibc e libstdc++ e i loro editor si aspettano librerie X11, fontconfig, freetype e simili.

Proposta: toolchain glibc fin dalla Fase 0 (vedi D-06); definire nella Fase 4 un **runtime per plugin** con elenco chiuso di librerie garantite (vedi M-09); decidere esplicitamente che il ponte verso plugin Windows è fuori perimetro, perché incompatibile con il modello di sicurezza e con le garanzie di tempo reale.

#### K-10 — cage: editor flottanti e due monitor

cage è un compositor a chiosco: massimizza le finestre principali. Gli editor VST3 sono finestre X11 che devono restare flottanti alla loro dimensione, e non possono essere incorporate in una superficie Wayland. Anche la gestione di due uscite con scala e frequenze diverse è limitata _(da verificare sulla versione corrente di cage)_.

Proposta: prova mirata all'inizio della Fase 3. Se cage non basta, anticipare un compositor Smithay minimo (due uscite, una superficie a schermo intero, finestre XWayland flottanti) invece di piegare cage.

#### K-11 — Avvio in meno di 3 secondi

Va definito l'intervallo: il POST di una scheda desktop dura da solo 6–15 secondi e non dipende da noi. Propongo di misurare **dall'ingresso in systemd-boot al primo fotogramma del menu**, con `systemd-analyze`. Budget _(stima)_: bootloader 0,1 s; kernel 0,5 s; sblocco TPM e LUKS 0,3–0,8 s; grafica e modalità video 0,3–0,5 s; systemd minimale 0,3 s; shell e inizializzazione Vulkan 0,3–0,6 s. Totale 1,8–2,8 s: fattibile, senza margine. L'enumerazione USB e la disponibilità dell'audio non devono bloccare il menu.

#### K-12 — Journal

Un journal che registra comandi e li riesegue all'apertura lega per sempre la lettura dei vecchi progetti alla logica dei comandi di allora. Cresce senza limite e una scrittura interrotta può lasciarlo troncato.

Proposta (specificata in 2.6): ogni record contiene sia il comando (intenzione, per cronologia e audit) sia le **modifiche primitive al modello** (per riesecuzione e annullamento); punti di ripristino periodici; segmenti con CRC; una transazione è un record solo.

#### K-13 — Sblocco con solo TPM e vecchie UKI firmate

Lo sblocco automatico protegge dal furto del solo disco, non del PC intero: chi ruba la macchina la avvia e arriva alla schermata di accesso con `/data/shared` già in chiaro. Le home restano protette da homed. Inoltre una UKI vecchia, firmata e vulnerabile, sblocca ancora il volume.

Proposta per la Fase 6: politica PCR firmata sulla misura della UKI (PCR 11) più PCR 7; protezione dal ritorno a versioni vecchie con `systemd-pcrlock` o contatore; TPM più PIN come opzione per chi vuole di più. Da decidere allora, non adesso.

#### K-14 — RTX 5080 «spenta»

Senza driver la scheda resta alimentata: consuma qualche decina di watt e le ventole possono girare a velocità fissa _(da verificare sul PC reale)_. Opzioni: disattivare lo slot dal firmware, mandarla in D3 via sysfs, o toglierla. Si misura nel passo P0.4 con un wattmetro o con `turbostat`.

### 1.3 Parti mancanti

#### M-01 — Compensazione della latenza dei plugin

Non compare in nessuna decisione. Sul percorso anticipativo si compensa leggendo in anticipo; sul percorso live no, quindi un plugin latente su un bus a valle allunga il monitoraggio. Proposta: latenza dichiarata per nodo nel piano (già prevista in 2.7), linee di ritardo inserite dal compilatore, e una modalità di monitoraggio a bassa latenza che esclude i plugin latenti a valle delle tracce live.

#### M-02 — Automazione, mappa del tempo e altre funzioni non nominate nel perimetro

Il perimetro dell'MVP non nomina: automazione dei parametri, mappa di tempo e metrica, marcatori, punch e registrazione a ciclo con take, gruppi e VCA, mandate e sidechain, congelamento delle tracce. Alcune sono probabilmente implicite in «mixer» ed «editor». L'automazione però condiziona il formato del piano, dei comandi e del progetto: va decisa prima della Fase 2 (vedi D-15).

#### M-03 — Modello del tempo e dei clock

Servono definizioni uniche per: posizione in campioni (intero a 64 bit), tempo musicale (propongo tick interi ad alta risoluzione, non virgola mobile), mappa del tempo, e la relazione tra `CLOCK_MONOTONIC` e il clock della scheda. Quest'ultima serve per allineare al campione gli eventi MIDI, che arrivano marcati con il tempo di sistema: propongo un anello ad aggancio di ritardo sui tempi di risveglio dei periodi, pubblicato in memoria condivisa.

#### M-04 — Stato persistente con radice in sola lettura

Con radice EROFS vanno decisi: dove vive `/var`; come si persiste `machine-id`; quali file di `/etc` sono modificabili (rete, nome host, fuso orario) e come. Proposta: `/etc` immutabile nell'immagine, stato in `/var` sulla partizione `state`, pochi file collegati verso `/var/lib/dawos/etc/`. Si implementa nel passo P0.5.

#### M-05 — Diagnostica: registri, crash, tracce

Dove finiscono i registri (journald su `state`, con limite di dimensione), i dump dei crash dei nostri processi, i messaggi del kernel dopo un blocco (`pstore` su variabili EFI), e le tracce del percorso live (anello senza lock per thread, letto da `dawos-health`).

#### M-06 — Definizione di xrun e di difetto

Propongo quattro contatori distinti: xrun di ALSA; scadenza del ciclo mancata dal motore; scadenza di un nodo mancata (silenzio e dissolvenza); discontinuità del segnale in loopback. I criteri di accettazione citano sempre quale contatore.

#### M-07 — Strategia di prova e backend audio alternativi

Il motore deve poter girare senza Scarlett: backend `null` (guidato da un timer) e `file` (ingresso e uscita su file, più veloce del tempo reale). Questo dà resa non in tempo reale deterministica, confrontabile con file di riferimento, e permette l'integrazione continua. Osservazione utile: l'esportazione è il percorso anticipativo con anticipo infinito; conviene progettarli come lo stesso codice.

#### M-08 — Fonte unica delle definizioni dei comandi

I comandi servono in cinque forme: tipi Rust, codifica sul socket, record del journal, strumenti MCP con schema JSON, voci della palette. Vanno generati da una definizione sola (un crate `dawos-commands` con derivazioni), altrimenti divergeranno.

#### M-09 — Runtime per plugin

Elenco chiuso e versionato delle librerie che un plugin può aspettarsi nell'immagine, più le regole della sandbox (nessun `io_uring`, nessuna rete, file solo tramite descrittori passati dalla sessione). Si definisce nella Fase 4, ma la scelta di glibc va fatta ora.

#### M-10 — Scollegamento della scheda e cambio di frequenza

Comportamento se la Scarlett sparisce a sessione aperta (passaggio al backend `null`, piano conservato, ripresa alla riconnessione) e sequenza per cambiare frequenza di campionamento (chiusura del dispositivo, nuovo piano). A 176,4 e 192 kHz la scheda riduce i canali disponibili _(da verificare sul dispositivo)_.

#### M-11 — Licenze

Il repository è sotto licenza MIT. VST3 SDK 3.8 è MIT _(verificato)_, restic è BSD, Smithay, wgpu ed egui sono MIT o Apache. `fcp-server` e i suoi file di descrizione dei dispositivi sono GPL _(da verificare la versione esatta)_: `dawos-fcpd` può usarli come riferimento per il protocollo, ma copiarne codice o dati imporrebbe la GPL a quel componente. Da decidere in D-08.

#### M-12 — Monitoraggio diretto tramite il mixer della Scarlett

Il mixer interno della scheda offre monitoraggio a latenza quasi nulla, che per una voce in cuffia è meglio di 7–9 ms. Il modulo «mix cuffie» potrebbe pilotarlo tramite `dawos-fcpd` invece di passare dal motore. È una scelta di Fase 5, ma influenza l'interfaccia di `dawos-fcpd`.

---

## Parte 2 — Specifica delle interfacce trasversali

Questa parte è una **proposta di specifica, versione 0**. Fissa forme e regole; i dettagli fini (elenco completo dei comandi, parametri dei nodi) si aggiungono nelle fasi che li usano. I punti che dipendono da una tua scelta rimandano alle domande della Parte 4.

### 2.1 Convenzioni comuni

**Due piani di comunicazione.** Tutto ciò che segue distingue:

- il **piano di controllo**: messaggi rari, strutturati, a lunghezza variabile, su socket Unix; codifica CBOR (vedi D-12);
- il **piano real-time**: record a dimensione fissa in memoria condivisa, senza serializzazione, letti e scritti dai thread live.

**Fiducia.** Ogni interfaccia dichiara chi si fida di chi.

| Processo | Livello | Conseguenza |
| --- | --- | --- |
| `dawos-session` | Fidato | Unico autore del modello e dei piani. |
| `dawos-engine` | Fidato | Convalida comunque ogni piano prima di accettarlo. |
| `dawos-shell` | Parzialmente fidato | Può solo inviare comandi e leggere. |
| `dawos-plughost` | Non fidato | Ogni dato che scrive in memoria condivisa è trattato come ostile. |
| `dawos-agent` | Non fidato | Comandi filtrati per capacità e modalità. |

**Identificativi.**

- Entità del modello: UUID versione 7 (128 bit, ordinabili per tempo). In forma testuale hanno un prefisso di tipo: `prj_`, `trk_`, `clp_`, `res_`, `plg_`, `cmd_`, `txn_`.
- Piano real-time: indici densi a 32 bit assegnati dal compilatore del piano. La corrispondenza con gli UUID è una tabella dentro il piano.
- Nessun percorso di file attraversa mai un'interfaccia: solo identificativi di risorsa o descrittori di file già aperti.

**Tempo.**

- `Frames`: intero senza segno a 64 bit, posizione assoluta in campioni alla frequenza del progetto.
- `Ticks`: intero con segno a 64 bit, tempo musicale. Proposta: 15 360 tick per quarto, divisibile per terzine, quintine e fino alla 1024-esima, e rappresentabile senza perdita in un MIDI Clip File (vedi D-20).
- Tempo di sistema: nanosecondi di `CLOCK_MONOTONIC`, solo per diagnostica e per la correlazione con il clock audio.

**Versioni.** Ogni formato porta una versione maggiore e una minore. Un lettore rifiuta una maggiore sconosciuta e ignora i campi sconosciuti di una minore più recente. Tutto è little-endian; l'unica architettura è x86-64.

### 2.2 Schema dei comandi

#### Struttura

Un comando è una busta con un carico tipizzato. La busta è identica sul socket, nel journal e, resa in JSON, negli strumenti MCP.

```rust
/// Envelope shared by every command, on every transport.
pub struct CommandEnvelope {
    /// Envelope schema version.
    pub v: u16,
    /// Unique id chosen by the issuer (UUIDv7); also the idempotency key.
    pub id: CommandId,
    /// Wall-clock time at the issuer, informational only.
    pub issued_at: Timestamp,
    /// Who is acting. Filled in by the session, never trusted from the client.
    pub author: Author,
    /// Which surface produced the command.
    pub origin: Origin,
    /// Transaction this command belongs to, if any.
    pub txn: Option<TxnId>,
    /// Model revision the issuer based its decision on (optimistic check).
    pub base_rev: Option<Revision>,
    /// Typed payload, named "<namespace>.<verb>".
    pub cmd: Command,
}

pub enum Author {
    User { user: UserId },
    Agent { user: UserId, agent_session: AgentSessionId },
    Device { surface: SurfaceId, user: UserId },
    System { service: ServiceName },
}

pub enum Origin { Shell, Palette, ControlSurface, Mcp, Script, Internal }
```

Ogni comando ha inoltre una descrizione statica, da cui si generano tipi, schema JSON, documentazione e voci della palette (vedi M-08).

```rust
pub struct CommandSpec {
    pub name: &'static str,          // e.g. "clip.move"
    pub since: u16,                  // schema version that introduced it
    pub class: CommandClass,         // Model | Transport | System
    pub undo: UndoPolicy,            // Undoable | Barrier | Skip
    pub capability: Capability,      // permission required from the author
    pub engine_effect: EngineEffect, // None | ParamChange | PlanSwap | Transport
}
```

Spazi dei nomi previsti per l'MVP: `project`, `track`, `clip`, `midi`, `mixer`, `route`, `plugin`, `transport`, `record`, `module`, `device`, `export`, `analysis`, `history`, `txn`, `control`.

Le **letture non sono comandi**: `query` e `subscribe` non passano dal journal e non hanno autore registrato. In MCP i comandi diventano strumenti e le letture diventano risorse.

Esempio reso in JSON:

```json
{
  "v": 1,
  "id": "cmd_0193285e-7c3a-7d1e-9a41-3f1c2b7a9e10",
  "issued_at": "2026-11-13T10:42:07.120Z",
  "author": { "kind": "user", "user": "usr_andrea" },
  "origin": "shell",
  "txn": null,
  "base_rev": 1041,
  "cmd": {
    "name": "clip.move",
    "args": { "clip": "clp_0193285a-11f0-7b3c-8d2e-5a6b7c8d9e0f", "to_track": null, "to_start": { "frames": 144000 } }
  }
}
```

#### Identità e autorizzazione

- La sessione ricava l'identità dal socket (`SO_PEERCRED`: l'utente di sistema del processo chiamante) e **sovrascrive** il campo `author`. Un client non può dichiararsi qualcun altro.
- Ogni comando richiede una capacità; ogni autore ne ha un insieme. L'agente ne ha un sottoinsieme e lavora su un ramo del progetto.
- La modalità (utente o agente) è un gettone posseduto da uno solo alla volta. Un comando da chi non lo possiede fallisce con `E_MODE`. Il comando `control.release` lo cede; solo un controllo fisico lo riprende.

#### Risultati ed errori

```rust
pub enum CommandOutcome {
    Applied {
        rev: Revision,            // model revision after the command
        created: Vec<EntityId>,   // ids of entities created by the command
    },
    Staged { txn: TxnId },        // accepted inside an open transaction
    Rejected(CommandError),
}

pub struct CommandError {
    pub code: ErrorCode,          // stable identifier, see table
    pub message: String,          // human-readable, Italian, not parsed by clients
    pub target: Option<EntityId>, // entity that caused the failure
    pub retryable: bool,
    pub details: Map<String, Value>,
}
```

| Famiglia | Codici | Significato |
| --- | --- | --- |
| Convalida | `E_SCHEMA`, `E_INVALID_ARG`, `E_NOT_FOUND`, `E_PRECONDITION` | Il comando è malformato o non applicabile allo stato attuale. |
| Concorrenza | `E_CONFLICT`, `E_TXN_UNKNOWN`, `E_TXN_ABORTED`, `E_DUPLICATE` | Revisione di base superata, transazione inesistente o annullata, identificativo già visto. |
| Autorizzazione | `E_DENIED`, `E_MODE` | Capacità mancante, o gettone di controllo non posseduto. |
| Risorse | `E_LIMIT`, `E_BUDGET`, `E_NO_MEMORY`, `E_IO` | Limite architetturale (R-03), budget real-time, memoria, disco. |
| Motore | `E_ENGINE_REJECTED`, `E_ENGINE_TIMEOUT` | Il motore ha rifiutato il piano o non ha risposto. |
| Interni | `E_INTERNAL` | Difetto nostro; sempre accompagnato da un rapporto. |

Un comando rifiutato non modifica nulla e **non** entra nel journal dei progetti; entra nel registro di audit se l'autore è l'agente.

#### Transazioni

1. `txn.begin { label }` restituisce un `TxnId`. Un autore ha al più una transazione aperta.
2. I comandi con quel `txn` vengono convalidati e applicati a una **copia in prova** del modello (strutture persistenti con condivisione strutturale, quindi a costo basso). Rispondono `Staged`.
3. `txn.commit` verifica che nessuna entità toccata sia cambiata dalla revisione di partenza; se sì fallisce con `E_CONFLICT`. Altrimenti produce **un solo record** di journal, un solo incremento di revisione e al più **un solo scambio di piano**.
4. `txn.abort`, la disconnessione del client o 5 secondi di inattività scartano la copia in prova.
5. La sessione elabora i comandi in un solo thread: l'ordine è totale e le transazioni sono serializzabili per costruzione.

Le transazioni sono brevi. Il lavoro lungo dell'agente con revisione non è una transazione ma un **ramo del progetto**, che si specifica nella fase dell'agente.

#### Annullamento

Ogni record di journal contiene le modifiche primitive con valore vecchio e nuovo (vedi 2.6). Annullare significa applicare le modifiche inverse come **nuovo record**, con riferimento a quello annullato; la cronologia non si riscrive mai. Per l'MVP propongo una cronologia lineare unica, con l'autore visibile su ogni voce (vedi D-21).

#### Applicazione al motore

| Effetto | Meccanismo | Momento |
| --- | --- | --- |
| `ParamChange` | Record nell'anello sessione → motore; il motore scrive il valore o la rampa nel blocco parametri del nodo. | Al confine del buffer successivo, o a un campione preciso. |
| `PlanSwap` | Nuovo piano compilato; protocollo in due fasi (vedi 2.3). | Al confine di un buffer, dopo la preparazione. |
| `Transport` | Record nell'anello. | Al confine di un buffer, o a una posizione indicata. |
| `None` | Solo modello e journal. | — |

Il comando risponde `Applied` quando il modello è aggiornato e il record è nel journal. La conferma del motore arriva come evento separato: l'interfaccia non aspetta l'audio per aggiornarsi.

### 2.3 Protocollo IPC tra sessione, motore e host

#### Canali

| Canale | Estremi | Trasporto | Uso |
| --- | --- | --- | --- |
| C1 | client ↔ sessione | Socket Unix `SOCK_SEQPACKET`, `/run/dawos/session.sock` | Comandi, risultati, letture, sottoscrizioni. |
| C2 | sessione ↔ motore | Coppia di socket creata da `dawosd` | Preparazione dei piani, passaggio di descrittori, ciclo di vita. |
| C3 | sessione ↔ motore | Due anelli SPSC in memoria condivisa | Record real-time applicati al confine di un buffer; eventi di ritorno. |
| C4 | motore ↔ host | Coppia di socket più memoria condivisa | Preparazione; ciclo audio per segmento. |
| C5 | sessione ↔ host | Coppia di socket | Stato dei plugin, elenco parametri, apertura dell'editor. |
| C6 | motore → shell, salute | Memoria condivisa in sola lettura | Misuratori e statistiche. |

`dawosd` crea le coppie di socket e le consegna ai processi all'avvio: nessun processo real-time apre socket per nome. La shell non parla mai con il motore; legge soltanto i misuratori.

#### Trama del piano di controllo

Ogni pacchetto `SOCK_SEQPACKET` è una trama: intestazione fissa di 16 byte più corpo CBOR. Dimensione massima 64 KiB; i corpi più grandi viaggiano in un `memfd` allegato.

```text
offset  dim  campo
0       4    magic "DWIP"
4       1    versione maggiore del protocollo
5       1    tipo: 1 HELLO, 2 REQ, 3 RES, 4 EVT, 5 ERR, 6 CANCEL
6       2    flag: bit 0 = descrittori allegati (SCM_RIGHTS)
                   bit 1 = il corpo è nel primo descrittore allegato
8       8    identificativo di correlazione
16      ..   corpo CBOR
```

Apertura: ciascun lato invia `HELLO { ruolo, versioni del protocollo, identificativo di build }`. Sui canali interni (C2, C4, C5) si pretende lo stesso identificativo di build, perché tutti i processi vengono dalla stessa immagine. Su C1 si negozia la versione, perché agente e strumenti possono essere più vecchi.

#### Record del piano real-time (C3)

Record fissi di 64 byte, una riga di cache.

```rust
#[repr(C, align(64))]
pub struct RtMsg {
    pub kind: u16,
    pub flags: u16,
    pub node: u32,        // dense node index in the current plan, or u32::MAX
    pub seq: u64,         // monotonically increasing per ring
    pub at_frame: u64,    // u64::MAX means "next buffer boundary"
    pub payload: [u8; 40],
}
```

| Verso | Tipi |
| --- | --- |
| Sessione → motore | `PLAN_COMMIT`, `PARAM_SET`, `PARAM_RAMP`, `TRANSPORT`, `ARM`, `UMP_INJECT`, `PANIC` |
| Motore → sessione | `PLAN_ACK`, `TRANSPORT_STATE`, `XRUN`, `CYCLE_OVERRUN`, `NODE_OVERRUN`, `HOST_ABANDONED`, `REC_PROGRESS`, `DEVICE_LOST` |

Se l'anello verso la sessione è pieno, il motore scarta l'evento e incrementa un contatore: il thread live non aspetta mai.

#### Scambio del piano in due fasi

1. La sessione compila il piano e lo scrive in uno slot libero della regione dei piani.
2. `PREPARE { slot, epoch }` su C2. Un thread **non** real-time del motore convalida il piano, alloca e inizializza lo stato dei nodi nuovi, apre i collegamenti con gli host, tocca tutte le pagine.
3. Il motore risponde `READY` oppure `ERR`. In caso di errore il comando fallisce con `E_ENGINE_REJECTED` e il modello non cambia.
4. La sessione scrive `PLAN_COMMIT { epoch }` su C3.
5. Il thread live, al confine del buffer successivo, sostituisce il puntatore al piano. In quel punto tutti i worker sono fermi, quindi non servono contatori di riferimenti.
6. Il motore emette `PLAN_ACK { epoch, at_frame }`. Dopo il periodo di grazia un thread non real-time libera le risorse del piano precedente.

#### Passaggio sincrono motore ↔ host (C4)

Per ogni segmento di catena esiste in memoria condivisa un blocco di controllo.

```rust
#[repr(C, align(128))]
pub struct HostSlot {
    /// Futex word. IDLE=0, REQUEST=1, RUNNING=2, DONE=3, ABANDONED=4, DEAD=5.
    pub state: AtomicU32,
    pub n_frames: u32,
    pub cycle: u64,       // written by the engine before REQUEST
    pub plan_epoch: u64,
    pub deadline_ns: u64, // CLOCK_MONOTONIC, informational for the host
    pub t_start_ns: u64,  // written by the host: untrusted statistics
    pub t_end_ns: u64,
}
```

Sequenza per ciclo, con worker del motore a priorità 80 e thread live dell'host a priorità 81 **sullo stesso core**:

```text
worker motore (FIFO 80)                    thread live host (FIFO 81)
-----------------------                    --------------------------
scrive ingressi ed eventi nel segmento     dorme in futex_wait(state)
state = REQUEST
futex_wake(state)  ───────────────────────► si sveglia e prelaziona il worker
   (prelazionato)                           state = RUNNING
                                            esegue la catena di plugin
                                            scrive le uscite
                                            state = DONE
   riprende ◄────────────────────────────── futex_wait(state)
legge state una sola volta: DONE
convalida le uscite (valori finiti)
state = IDLE
```

- Il worker fa una chiamata di sistema limitata (`futex_wake`); l'host una (`futex_wait`). Nessuna attesa attiva tra processi.
- **Scadenza.** Un thread watchdog del motore (priorità 95, core non live) ha un timer per ciclo. Se alla scadenza lo stato è `REQUEST` o `RUNNING`, lo porta ad `ABANDONED` e declassa o termina l'host. Il worker riprende, vede `ABANDONED`, emette la dissolvenza dall'ultimo buffer valido, e segnala `HOST_ABANDONED`. `dawosd` riavvia l'host.
- **Uscite non fidate.** Il worker legge lo stato una volta sola, usa solo offset fissati alla preparazione, e sostituisce con zero ogni valore non finito prima che arrivi a un bus.
- Sul lato anticipativo vale lo stesso schema con priorità 40 e 41 e blocchi più lunghi.

L'assegnazione dei segmenti ai core la fa il compilatore del piano, usando i costi misurati (vedi 2.7). I nodi nativi restano a schedulazione dinamica tra i worker.

#### Vitalità

- Il motore **non dipende dalla sessione** per continuare: se la sessione muore, il motore tiene l'ultimo piano, continua a suonare e chiude in modo sicuro i file in registrazione.
- `dawosd` riavvia la sessione, che si ricollega, confronta l'epoca del piano e riprende.
- Se muore il motore, `dawosd` lo riavvia e la sessione ripete preparazione e scambio. Obiettivo: audio di nuovo attivo entro un secondo _(stima)_.

### 2.4 Formato della memoria condivisa

#### Regole generali

- Ogni regione è un `memfd` creato dal lato fidato, dimensionato una volta e sigillato contro il ridimensionamento (`F_SEAL_SHRINK`, `F_SEAL_GROW`, `F_SEAL_SEAL`). Ai lettori puri si consegna con `F_SEAL_FUTURE_WRITE` e mappatura in sola lettura.
- Si mappa una volta, alla preparazione, con pagine già popolate e bloccate in RAM. Nessuna mappatura cambia mentre l'audio gira.
- Per i pool audio propongo pagine da 2 MB riservate al boot (`hugetlb`), dimensionate dal profilo _(dimensione da misurare)_.
- Ogni atomico condiviso tra un lato che scrive e uno che legge sta su righe di cache separate; allineamento a 128 byte per evitare anche il prefetch della riga adiacente.
- Gli offset sono relativi all'inizio della regione; nessun puntatore attraversa un confine di processo.

#### Intestazione di regione

```text
offset  dim  campo
0x00    8    magic "DAWOSHM\0"
0x08    4    versione del layout
0x0C    4    tipo di regione
0x10    8    dimensione totale in byte
0x18    8    epoca (identifica la creazione)
0x20    4    numero di sezioni
0x24    4    CRC32C dell'intestazione e della tabella
0x40    ..   tabella delle sezioni: { tipo u32, flag u32, offset u64, lunghezza u64 }
```

| Tipo | Regione | Chi scrive | Chi legge |
| --- | --- | --- | --- |
| 1 | Controllo real-time (anelli C3) | Sessione e motore | Motore e sessione |
| 2 | Piani (2–3 slot) | Sessione | Motore |
| 3 | Segmento di host (controllo, audio, eventi) | Motore e host | Host e motore |
| 4 | Misuratori | Motore | Shell |
| 5 | Statistiche e tracce | Motore | `dawos-health`, sessione |
| 6 | Base dei tempi (clock audio ↔ `CLOCK_MONOTONIC`) | Motore | Tutti |

#### Anello SPSC

```rust
#[repr(C, align(128))]
pub struct RingProducer { pub head: AtomicU64 }   // written by the producer only

#[repr(C, align(128))]
pub struct RingConsumer {
    pub tail: AtomicU64,                          // written by the consumer only
    pub parked: AtomicU32,                        // futex word, non-real-time side only
}

#[repr(C)]
pub struct RingLayout {
    pub capacity: u32,      // power of two, fixed at creation
    pub record_size: u32,   // bytes, multiple of 64
    // followed by RingProducer, RingConsumer, then capacity * record_size bytes
}
```

Gli indici crescono senza mai tornare a zero e si mascherano alla lettura. Il lato fidato **non si fida** degli indici dell'altro: li legge una volta, controlla che la differenza stia tra zero e la capacità, e solo allora li usa.

#### Segmento di host

Layout fissato alla preparazione e non più modificabile: un `HostSlot`, poi i buffer audio planari a 32 bit in virgola mobile (ingressi, uscite, eventuali sidechain) per il numero massimo di campioni del quanto, poi due code di eventi (in ingresso e in uscita) con pacchetti UMP e cambi di parametro marcati con l'offset in campioni.

#### Triplo buffer per i misuratori

Tre copie della struttura dei misuratori e un byte atomico che codifica quale copia è «pronta» e se è nuova. Il motore scrive nella propria copia e la scambia con quella pronta con una sola operazione atomica; la shell scambia la propria con quella pronta quando è nuova. Nessuno aspetta nessuno, e la shell vede sempre un'istantanea coerente.

### 2.5 Manifest dei moduli

Un file TOML per modulo, compilato nell'immagine in `/usr/lib/dawos/modules/<id>/module.toml`. Il progetto usa JSON perché è un formato di scambio; qui propongo TOML perché questi file si scrivono a mano.

```toml
[module]
id = "dawos.eq"
version = "1.0.0"
api = 1                       # module API version expected from the core
title = "EQ parametrico"
requires = ["dawos.mixer >= 1.0"]
profiles = ["recording", "mixing", "mastering"]

[[node]]
type = "dawos.eq.parametric"
kind = "effect"
channels = "any"              # channel layouts accepted
latency_frames = 0
supports_snapshot = true      # state can be copied for live promotion
state_bytes = 4096
# Measured by the calibration harness at build time, not hand-written.
cost_ns_per_frame = { p_core = 9, e_core = 16 }

[[node.param]]
id = "band1.freq"
unit = "Hz"
range = [20.0, 20000.0]
default = 100.0
scale = "log"
smoothing_ms = 5.0

[[panel]]
id = "dawos.eq.editor"
views = ["mix", "editing"]

[[command]]
namespace = "eq"

[resources]
memory_mb = 8
hugepages_2m = 0
```

| Sezione | Contenuto | A cosa serve |
| --- | --- | --- |
| `module` | Identità, versione, dipendenze, profili | Menu dei moduli e risoluzione delle dipendenze. |
| `node` | Tipi di nodo, latenza, stato, costo misurato | Compilatore del piano e verifica del budget prima di attivare. |
| `node.param` | Parametri con unità, intervallo, scala | Comandi, automazione, superfici di controllo, MCP. |
| `panel` | Pannelli e viste in cui compaiono | Shell. |
| `command` | Spazi dei nomi dei comandi forniti | Livello comandi e palette. |
| `process` | Processi propri del modulo, se ne ha | `dawosd`: avvio, sandbox, limiti. |
| `resources` | Memoria, pagine grandi, file | Verifica del budget. |

Ciclo di vita: attivare o disattivare un modulo è un comando (`module.activate`), che produce uno scambio di piano; il periodo di grazia è quello dello scambio in due fasi.

### 2.6 Progetto: cartella, `manifest.json` e journal

#### Struttura della cartella

```text
Brano.dawos/
├── manifest.json            identità, requisiti, puntatori
├── journal/
│   ├── 000001.dwj           segmenti append-only
│   ├── 000002.dwj
│   └── checkpoints/
│       └── 0000001000.cbor.zst   istantanea del modello alla revisione 1000
├── audio/                   BWF/RF64 immutabili, nominati per identificativo di risorsa
├── midi/                    MIDI Clip File, immutabili per versione
├── plugins/                 stati dei plugin, immutabili per versione
├── cache/                   picchi, analisi, rese: sempre rigenerabile
└── .lock                    blocco di apertura esclusiva
```

Regola di base: **il journal è la verità, `manifest.json` è la porta d'ingresso**. Il manifest si riscrive solo ai punti di ripristino, in modo atomico (file temporaneo, `fsync`, rinomina, `fsync` della cartella).

#### Schema di `manifest.json`

```json
{
  "format": "dawos-project",
  "format_version": { "major": 1, "minor": 0 },
  "project_id": "prj_01932850-0000-7000-8000-000000000001",
  "name": "Brano",
  "created": "2026-11-13T10:00:00Z",
  "written_by": { "dawos": "0.1.0", "build": "a1b2c3d4" },
  "sample_rate": 96000,
  "ticks_per_quarter": 15360,
  "head": {
    "revision": 1042,
    "segment": "journal/000002.dwj",
    "checkpoint": "journal/checkpoints/0000001000.cbor.zst"
  },
  "requires": {
    "modules": [{ "id": "dawos.mixer", "min_version": "1.0.0" }],
    "plugins": [{ "format": "vst3", "class_id": "ABCDEF0123456789ABCDEF0123456789", "name": "Esempio", "vendor": "Esempio" }]
  }
}
```

| Campo | Obbligatorio | Nota |
| --- | --- | --- |
| `format`, `format_version` | Sì | Un lettore rifiuta una versione maggiore sconosciuta. |
| `project_id` | Sì | Non cambia mai, nemmeno copiando la cartella con reflink; le copie ricevono un nuovo identificativo con un comando esplicito. |
| `sample_rate`, `ticks_per_quarter` | Sì | Fissati alla creazione. |
| `head` | Sì | Suggerimento per aprire in fretta; in caso di dubbio vince la scansione del journal. |
| `requires` | Sì | Permette di dire cosa manca prima di aprire. |

L'elenco delle risorse (file audio, MIDI, stati) **non** sta nel manifest: è parte del modello e quindi del journal e delle istantanee.

#### Formato del journal

Un segmento è un file con un'intestazione e una sequenza di record.

```text
Intestazione di segmento (64 byte)
offset  dim  campo
0       4    magic "DWJ1"
4       2    versione maggiore
6       2    versione minore
8       16   identificativo del progetto
24      8    numero di sequenza del segmento
32      8    prima revisione contenuta
40      16   concatenazione finale del segmento precedente
56      4    riservato
60      4    CRC32C dell'intestazione

Record
offset  dim  campo
0       4    lunghezza del corpo in byte
4       4    CRC32C dei byte 8..fine del corpo
8       1    tipo: 1 COMMIT, 2 CHECKPOINT, 3 SESSION, 4 MARK
9       1    flag
10      6    riservato, zero
16      8    revisione del modello dopo il record
24      16   concatenazione: BLAKE3 troncato di (concatenazione precedente ‖ corpo)
40      ..   corpo CBOR, riempito a multipli di 8 byte
```

Un record `COMMIT` contiene un comando singolo oppure un'intera transazione. Corpo, reso in JSON:

```json
{
  "author": { "kind": "user", "user": "usr_andrea" },
  "origin": "shell",
  "time": "2026-11-13T10:42:07.120Z",
  "txn": null,
  "undo_of": null,
  "commands": [
    { "id": "cmd_0193285e-7c3a-7d1e-9a41-3f1c2b7a9e10", "name": "clip.move", "args": { "to_start": { "frames": 144000 } } }
  ],
  "delta": [
    { "op": "set", "entity": "clp_0193285a-11f0-7b3c-8d2e-5a6b7c8d9e0f", "field": "start", "old": { "frames": 96000 }, "new": { "frames": 144000 } }
  ]
}
```

Regole:

- **Comando e modifiche insieme.** `commands` è l'intenzione, per cronologia e audit. `delta` è l'elenco delle modifiche primitive al modello: `create`, `delete`, `set`, `list_insert`, `list_remove`, `list_move`, ciascuna con il valore vecchio. All'apertura si applicano i `delta`, non si rieseguono i comandi: i vecchi progetti restano leggibili anche quando la logica dei comandi cambia.
- **Una transazione, un record.** Nel journal non esistono transazioni a metà.
- **Durata.** Scrittura in coda con `fdatasync` raggruppato ogni 100 ms al massimo; sincronizzazione immediata all'inizio e alla fine di una registrazione e sui marcatori.
- **Recupero.** Si scorre il segmento; al primo record con lunghezza impossibile o CRC errato ci si ferma e si tronca lì. Si perde al massimo l'ultimo gruppo non sincronizzato.
- **Punti di ripristino.** Ogni 1000 revisioni o alla chiusura: istantanea completa del modello, compressa, e un record `CHECKPOINT`. L'apertura carica l'ultima istantanea valida e applica i record successivi.
- **Rotazione.** Nuovo segmento a 64 MiB o a ogni punto di ripristino. I segmenti vecchi restano, perché sono l'annullamento persistente; un comando esplicito compatta la cronologia.
- **Concatenazione.** Ogni record include l'impronta del precedente: una modifica a posteriori del journal è rilevabile. Costa poco e serve all'audit delle azioni dell'agente.
- **Dati voluminosi.** Audio, MIDI registrato e stati dei plugin non entrano nel journal: vanno in file immutabili e il journal ne cita l'identificativo. Le modifiche di singole note sono `delta` sul modello; a ogni punto di ripristino la clip viene riscritta come nuova versione del MIDI Clip File.

### 2.7 Struttura del piano di esecuzione

Il piano è un blocco di byte **immutabile e indipendente dalla posizione**, scritto dal compilatore della sessione in uno slot della regione dei piani. Contiene solo indici e offset. Il motore lo convalida per intero nella fase di preparazione: indici entro i limiti, grafo senza cicli, buffer non sovrapposti in modo scorretto.

```rust
#[repr(C)]
pub struct PlanHeader {
    pub magic: [u8; 8],        // "DWPLAN\0\0"
    pub version: u32,
    pub flags: u32,
    pub epoch: u64,            // strictly increasing
    pub model_rev: u64,        // model revision this plan was compiled from
    pub sample_rate: u32,
    pub quantum: u32,          // frames per live cycle
    pub ahead_block: u32,      // frames per anticipative block
    pub ahead_frames: u32,     // look-ahead of the anticipative domain
    pub n_tasks: u32,
    pub n_live_tasks: u32,     // tasks [0, n_live_tasks) belong to the live domain
    pub n_buffers: u32,
    pub n_segments: u32,       // external host segments
    pub n_meet_points: u32,
    pub tables: [TableRef; 8], // offset + length of each table below
    pub crc32c: u32,
}

#[repr(C)]
pub struct TaskDesc {
    pub node: u32,             // dense node index; maps to an EntityId in the id table
    pub kind: u16,             // Native, HostSegment, Sum, Delay, MeetRead, MeetWrite, DeviceIn, DeviceOut
    pub domain: u8,            // 0 = live, 1 = anticipative
    pub core_hint: u8,         // fixed core for HostSegment, 0xFF = any worker
    pub n_deps: u32,           // number of predecessors: initial value of the pending counter
    pub succ: Span,            // successors, in the successor table
    pub inputs: Span,          // buffer indices
    pub outputs: Span,
    pub params: u32,           // parameter block index (lives outside the plan)
    pub state: u32,            // state slot index (lives outside the plan)
    pub latency: u32,          // frames reported by the node
    pub cost_ns: u32,          // estimated cost per cycle, from measurements
}
```

Tabelle del piano:

| Tabella | Contenuto |
| --- | --- |
| Attività | `TaskDesc` in ordine topologico; prima il dominio live, poi l'anticipativo. |
| Successori | Indici di attività, per i contatori di dipendenze. |
| Buffer | Per ogni buffer logico: indice del buffer fisico nel pool, canali, formato (32 bit, o 64 bit per gli accumulatori dei bus). |
| Segmenti | Per ogni segmento esterno: regione di memoria condivisa, core, priorità, elenco dei plugin. |
| Punti d'incontro | Anelli tra anticipativo e live: capacità, anticipo, ritardo di compensazione. |
| Identificativi | Indice di nodo → UUID, per diagnostica ed eventi. |
| Compensazione | Ritardi inseriti dal compilatore per allineare i percorsi (vedi M-01). |
| Trasferimento di stato | Per ogni nodo che cambia dominio: istantanea o dissolvenza. |

Principi:

- **Lo stato sta fuori dal piano.** Stato dei nodi e blocchi dei parametri vivono in aree del motore indicizzate da identificativi stabili, e sopravvivono allo scambio. Girare una manopola non richiede un nuovo piano.
- **I buffer fisici si riusano.** Il compilatore assegna i buffer logici a pochi buffer fisici con un'analisi di vita, come un allocatore di registri: l'impronta in cache resta piccola anche con mille tracce.
- **Esecuzione di un ciclo.** Il thread che aspetta ALSA copia i contatori iniziali in un vettore per ciclo, mette in coda le attività senza predecessori e sveglia i worker. Ogni worker esegue un'attività, decrementa i contatori dei successori e mette in coda quelli arrivati a zero. I segmenti esterni vanno nella coda del loro core.
- **Promozione.** Passare una traccia da anticipativo a live è uno scambio di piano in cui alcune attività cambiano dominio; la tabella di trasferimento dice come portare lo stato.
- **Dimensioni.** Con i limiti di R-03 (1024 tracce × 64 slot) il caso peggiore è nell'ordine di 70–80 mila attività: gli indici a 32 bit bastano, e un piano pesa pochi megabyte _(stima)_. La compilazione deve restare sotto qualche decina di millisecondi per non rendere lenta l'interfaccia; se non basta, si passa alla compilazione incrementale.

---

## Parte 3 — Piano dettagliato della Fase 0

### 3.1 Obiettivo e prodotti

**Obiettivo**: un'immagine dawOS che si avvia in QEMU e sul 285K, con kernel 6.18 real-time, radice EROFS verificata, slot A/B, UKI firmata con chiave di sviluppo e core partizionati; e la prova misurata che i core live rispettano la soglia di 50 µs per 24 ore sotto carico.

**Prodotti alla chiusura**:

- l'albero `BR2_EXTERNAL` e gli script che costruiscono l'immagine con un solo comando;
- il rapporto di misura in `docs/misure/fase-0/`, con dati grezzi e configurazioni;
- i registri delle decisioni (ADR) per le scelte D-01…D-09;
- un'etichetta git `fase-0`.

Nella Fase 0 non si scrive codice di prodotto: solo configurazione, script di costruzione e uno script di regolazione che `dawosd` sostituirà più avanti.

### 3.2 Struttura del repository

```text
daw-os/
├── README.md
├── LICENSE
├── Makefile                      punti d'ingresso: image, qemu, flash, measure, lint
├── docs/
│   ├── 00-revisione-specifiche-fase0.md      questo documento
│   ├── adr/                      una decisione per file, numerata
│   ├── fasi/                     piano e chiusura di ogni fase
│   └── misure/fase-0/            rapporti, dati grezzi, grafici
├── buildroot/                    sottomodulo git fissato a un tag 2026.02.x
├── external/                     albero BR2_EXTERNAL (nome: DAWOS)
│   ├── external.desc
│   ├── external.mk
│   ├── Config.in
│   ├── configs/
│   │   └── dawos_x86_64_dev_defconfig
│   ├── board/dawos/x86_64/
│   │   ├── linux/
│   │   │   ├── base.fragment     piattaforma x86-64 minima
│   │   │   ├── rt.fragment       PREEMPT_RT, tick, RCU, tracer
│   │   │   ├── hw-285k.fragment  driver del PC reale
│   │   │   ├── qemu.fragment     virtio
│   │   │   ├── storage.fragment  NVMe, dm-verity, EROFS, XFS
│   │   │   ├── audio.fragment    USB audio, UMP, hwdep
│   │   │   └── debug.fragment    lockdep e simili, mai nelle misure
│   │   ├── cmdline/
│   │   │   ├── common.txt
│   │   │   ├── profile-285k.txt
│   │   │   └── profile-generic.txt
│   │   ├── repart.d/             definizioni delle partizioni
│   │   ├── loader/               configurazione di systemd-boot
│   │   ├── rootfs-overlay/       unità systemd, sysctl, tmpfiles
│   │   ├── post-build.sh
│   │   └── post-image.sh         verity, UKI, firma, immagine disco
│   ├── package/
│   │   ├── dawos-platform/       script di regolazione e topologia
│   │   └── dawos-measure/        script di misura e di carico
│   └── patches/                  patch a pacchetti di Buildroot, se servono
├── crates/                       spazio di lavoro Rust, vuoto fino alla Fase 1
├── tools/                        script lato host: qemu, chiavi, flash, grafici
└── .github/workflows/            controlli: markdownlint, shellcheck, defconfig
```

Scelte da notare:

- **Buildroot come sottomodulo fissato**, non scaricato al volo: la costruzione è riproducibile e l'aggiornamento è un commit esplicito.
- **Una sola configurazione, più frammenti del kernel.** La stessa immagine si avvia in QEMU e sul PC reale; le differenze stanno nei profili della riga di comando.
- **Le chiavi non entrano mai nel repository.** `tools/` le genera in una cartella ignorata da git.

### 3.3 Schema del disco

GPT sull'NVMe da 1 TB. Le dimensioni di `shared` e `homes` dipendono da D-04.

| N. | Etichetta | Dimensione | Contenuto |
| --- | --- | --- | --- |
| 1 | `esp` | 1 GiB | systemd-boot e le UKI, due o tre versioni. |
| 2 | `dawos-root-<versione>` | 4 GiB | Slot A: EROFS. |
| 3 | `dawos-verity-<versione>` | 128 MiB | Slot A: albero di hash. |
| 4 | `_empty` → slot B | 4 GiB | Slot B, scritto dal primo aggiornamento. |
| 5 | `_empty` → slot B | 128 MiB | Slot B: albero di hash. |
| 6 | `state` | 32 GiB | `/var`. Nella Fase 0 ext4 in chiaro; LUKS2 con TPM dalla Fase 6. |
| 7 | `shared` | 250 GiB _(proposta)_ | XFS con reflink: `/data/shared`. |
| 8 | `homes` | il resto | XFS che ospiterà le immagini di homed. |

L'etichetta delle partizioni di sistema contiene la versione: è lo schema che `systemd-sysupdate` usa per scegliere lo slot da sovrascrivere, e permette alla riga di comando firmata di indicare la propria radice senza sapere se è nello slot A o B. Lo slot di 4 GiB è largo di proposito: l'immagine iniziale starà in poche centinaia di megabyte _(stima)_.

### 3.4 Passi

Ogni passo ha un criterio di accettazione verificabile. Un passo non si considera chiuso finché il criterio non è stato eseguito e annotato.

#### P0.1 — Ambiente di sviluppo

**Cosa**: preparare WSL2 per Buildroot e QEMU.

```ini
# /etc/wsl.conf (dentro la distribuzione)
[interop]
appendWindowsPath=false

[boot]
systemd=true
```

```ini
# %UserProfile%\.wslconfig (su Windows)
[wsl2]
nestedVirtualization=true
memory=48GB
processors=20
```

```bash
sudo apt install build-essential git rsync bc cpio unzip file wget python3 \
  libncurses-dev libssl-dev qemu-system-x86 ovmf swtpm sbsigntool \
  cryptsetup-bin erofs-utils mtools dosfstools shellcheck
```

Punti delicati:

- Buildroot rifiuta un `PATH` con spazi: per questo `appendWindowsPath=false`.
- Il repository va clonato nel file system di Linux (`~/`), mai sotto `/mnt/c`: lì la costruzione è lenta e le differenze tra maiuscole e minuscole la rompono.
- La virtualizzazione annidata serve per avere `/dev/kvm` dentro WSL2.

**Accettazione**: `ls /dev/kvm` esiste; `echo "$PATH"` non contiene spazi; `make -C buildroot help` funziona.

#### P0.2 — Scheletro e primo avvio in QEMU

**Cosa**: sottomodulo Buildroot, albero `external/` minimo, configurazione con toolchain glibc, systemd, kernel 6.18 con configurazione predefinita, avvio UEFI in QEMU con OVMF e console seriale. Niente verity né firma, per ora.

Opzioni principali della configurazione _(nomi da verificare sul tag reale)_:

```text
BR2_x86_64=y
BR2_x86_x86_64_v3=y
BR2_TOOLCHAIN_EXTERNAL=y
BR2_TOOLCHAIN_EXTERNAL_BOOTLIN=y
BR2_TOOLCHAIN_EXTERNAL_BOOTLIN_X86_64_V3_GLIBC_STABLE=y
BR2_INIT_SYSTEMD=y
BR2_LINUX_KERNEL=y
BR2_LINUX_KERNEL_CUSTOM_VERSION=y
BR2_LINUX_KERNEL_CUSTOM_VERSION_VALUE="6.18.<ultima>"
BR2_LINUX_KERNEL_CONFIG_FRAGMENT_FILES="<elenco dei frammenti>"
BR2_TARGET_ROOTFS_EROFS=y
BR2_PACKAGE_SYSTEMD_BOOT=y
BR2_ROOTFS_OVERLAY="$(BR2_EXTERNAL_DAWOS_PATH)/board/dawos/x86_64/rootfs-overlay"
BR2_ROOTFS_POST_IMAGE_SCRIPT="$(BR2_EXTERNAL_DAWOS_PATH)/board/dawos/x86_64/post-image.sh"
```

```bash
make image    # equivale a: make -C buildroot BR2_EXTERNAL=../external O=../output dawos_x86_64_dev_defconfig all
make qemu     # avvia l'immagine con OVMF, KVM e console seriale
```

Il livello `x86-64-v3` dà AVX2 e FMA a tutto lo spazio utente, coerente con la decisione sul motore.

**Accettazione**: da una copia pulita del repository, `make image && make qemu` arriva a un prompt sulla console seriale; `systemd-analyze` non segnala unità fallite.

#### P0.3 — Kernel 6.18 real-time

**Cosa**: sostituire la configurazione predefinita con i frammenti. Opzioni essenziali _(da verificare una per una con `make linux-menuconfig`)_:

| Area | Opzioni |
| --- | --- |
| Tempo reale | `PREEMPT_RT`, `HZ_1000`, `NO_HZ_FULL`, `RCU_NOCB_CPU`, `CPU_ISOLATION`, `HIGH_RES_TIMERS` |
| Topologia | `SCHED_MC`, `SCHED_CLUSTER`, `X86_INTEL_PSTATE`, `INTEL_IDLE`, `CPUSETS`, `CGROUPS` |
| Misura | `OSNOISE_TRACER`, `TIMERLAT_TRACER`, `HWLAT_TRACER`, `FTRACE`, `SCHED_DEBUG`, `X86_MSR` |
| Avvio e radice | `EFI_STUB`, `BLK_DEV_NVME`, `DM_VERITY`, `DM_INIT`, `EROFS_FS`, `XFS_FS`, tutti integrati e non come moduli |
| Sicurezza (predisposizione) | `TCG_TPM`, `TCG_CRB`, `TCG_TIS`, `DM_CRYPT`, `SECURITY_LANDLOCK`, `IO_URING` |
| Audio (predisposizione) | `SND_USB_AUDIO`, `SND_USB_AUDIO_MIDI_V2`, `SND_UMP`, `SND_HWDEP`, `USB_XHCI_HCD` |
| Grafica (predisposizione) | `DRM_I915`; `DRM_XE` come modulo per confronto |
| Da spegnere | `NUMA_BALANCING`, `KSM`, pagine grandi trasparenti a `madvise`, ogni opzione `DEBUG_*` pesante, `PROVE_LOCKING` fuori dal frammento di debug |

**Accettazione**: `uname -v` contiene `PREEMPT_RT`; `cat /sys/kernel/realtime` dà `1`; `rtla timerlat top -d 10s` gira in QEMU (i numeri in QEMU non contano).

#### P0.4 — Primo avvio sul 285K e rumore di fondo del firmware

Questo passo viene **prima** del lavoro su verity e firma, perché affronta il rischio K-01.

**Cosa**:

1. Portare l'immagine sull'NVMe (metodo secondo D-01) e avviarla con Secure Boot ancora disattivato.
2. Rilevare la topologia e archiviarla: `lscpu -e`, `/sys/devices/cpu_core/cpus`, `/sys/devices/cpu_atom/cpus`, condivisione della L2 da `/sys/devices/system/cpu/cpu*/cache/index2/shared_cpu_list`, `lspci -nn`, `lsusb -t`, `/proc/interrupts`.
3. Impostare il firmware. Lista di partenza: grafica primaria integrata; emulazione USB legacy disattivata; CSM disattivato; gestione del risparmio energetico lasciata al sistema operativo; ASPM disattivato; illuminazione e controller accessori disattivati; funzioni di regolazione dinamica del produttore disattivate; profilo di memoria stabile. Ogni voce cambiata si annota.
4. Misurare il rumore hardware su tutti i core, senza alcun isolamento, per almeno 4 ore:

   ```bash
   rtla hwnoise -d 4h
   turbostat --quiet --interval 5 --show CPU,Busy%,Bzy_MHz,SMI,CoreTmp,PkgWatt --out turbostat.csv
   ```

5. Misurare consumo e rumore con la RTX 5080 nelle tre condizioni di K-14.

**Accettazione**: rumore hardware massimo ≤ 15 µs su ogni core; colonna SMI di `turbostat` ferma per tutta la prova; topologia archiviata in `docs/misure/fase-0/topologia.md`. Se il criterio non passa, **ci si ferma qui** e si decide insieme come procedere.

#### P0.5 — Catena di avvio: verity, UKI, firma, A/B

**Cosa**, in `post-image.sh`:

1. Calcolare l'albero di hash della radice e la sua impronta:

   ```bash
   veritysetup format --root-hash-file=root.hash rootfs.erofs rootfs.verity
   ```

2. Comporre la riga di comando con l'impronta. Senza initrd (vedi D-03) il kernel crea da solo il dispositivo verificato:

   ```text
   dm-mod.create="root,,,ro,0 <settori> verity 1 PARTLABEL=dawos-root-<v> PARTLABEL=dawos-verity-<v> 4096 4096 <blocchi> 1 sha256 <impronta> <sale>"
   root=/dev/dm-0 ro rootwait
   ```

   L'uso di `PARTLABEL` nella tabella di `dm-mod.create` è _da verificare_; la documentazione mostra percorsi `/dev` e coppie maggiore:minore. Se non funziona, si ripiega su un initrd minimo.

3. Costruire e firmare la UKI con i due profili (`285k`, `generico`):

   ```bash
   ukify build --linux=bzImage --cmdline=@cmdline-generic.txt --os-release=@os-release \
     --secureboot-private-key=keys/db.key --secureboot-certificate=keys/db.crt \
     --output="dawos_${VERSION}+3-0.efi"
   ```

   Il secondo profilo si costruisce a parte e si unisce con `--join-profile`. Il suffisso `+3-0` è il contatore dei tentativi di systemd-boot. Se il pacchetto systemd di Buildroot non fornisce `ukify` per l'host _(da verificare)_, lo si aggiunge in `external/package/`.

4. Comporre l'immagine del disco con `systemd-repart` secondo `repart.d/`.
5. Predisporre lo stato persistente (M-04): `/var` su `state`, `machine-id` conservato, registri di journald con limite.
6. In QEMU: generare chiavi di sviluppo, iscriverle nelle variabili di OVMF, avviare con Secure Boot attivo. Sul 285K: aggiungere il certificato di sviluppo a `db` dal menu del firmware, senza togliere le chiavi esistenti.

**Accettazione**:

- in QEMU e sul 285K, `bootctl status` riporta Secure Boot attivo e la UKI avviata;
- `veritysetup status root` riporta `verified`; `mount` mostra la radice in sola lettura;
- **prova di manomissione**: alterando un byte della partizione radice, la lettura fallisce e il sistema non completa l'avvio;
- **prova di ritorno**: con uno slot B valido e lo slot A manomesso, dopo tre tentativi systemd-boot avvia lo slot B; dopo un avvio riuscito il contatore viene rimosso da `systemd-bless-boot`;
- una UKI non firmata viene rifiutata dal firmware.

#### P0.6 — Partizione dei core e riduzione del rumore

**Cosa**. Riga di comando del profilo `285k`, con la mappa proposta in C-03 _(numeri di CPU da confermare con la topologia rilevata in P0.4)_:

```text
nohz_full=2-7 rcu_nocbs=2-7 isolcpus=domain,managed_irq,4-7 irqaffinity=20-23
nowatchdog nmi_watchdog=0 nosoftlockup skew_tick=1 tsc=reliable
transparent_hugepage=madvise
```

Lo script `dawos-tune`, eseguito da un'unità systemd all'avvio:

1. legge la topologia da `/sys`, calcola i quattro domini e scrive `/run/dawos/topology.json`;
2. confronta i domini con la riga di comando effettiva e segnala le differenze;
3. sposta gli interrupt: ogni IRQ sul cluster E4, salvo quelli per CPU; le code dell'NVMe restano «gestite» dal kernel, che con `managed_irq` evita i core isolati;
4. limita le workqueue non vincolate ai core non live (`/sys/devices/virtual/workqueue/cpumask`);
5. imposta `vm.stat_interval=120`, `kernel.timer_migration`, e il _fair server_ sui core live secondo l'esito delle prove (K-04);
6. imposta il PM QoS per CPU sui core live (`/sys/devices/system/cpu/cpuN/power/pm_qos_resume_latency_us`);
7. fissa la frequenza dei core live con `intel_pstate` (minimo uguale al massimo, preferenza energetica su prestazioni).

**Esperimenti inclusi in questo passo**, ciascuno con una misura breve prima e dopo:

- polling contro C1 sui core live: latenza, temperatura, potenza (D-09);
- _fair server_ predefinito contro disattivato, con un thread `SCHED_FIFO` che gira a vuoto (K-04);
- verifica di C-02: un cgroup _threaded_ può essere radice di una partizione isolata? `sched_setaffinity` attraversa il confine?
- `i915` contro `xe` come sorgente di rumore sul cluster di sistema.

**Accettazione**, con una prova di 10 minuti sotto carico:

- su ogni core live, la differenza di `/proc/interrupts` è nulla tranne che per il timer locale;
- una traccia `sched_switch` sui core live mostra solo il thread di prova e i thread del kernel per CPU attesi;
- `rtla osnoise top -c 4-7 -d 10m` riporta rumore massimo singolo ≤ 10 µs;
- `dawos-tune` è idempotente e il suo esito è leggibile in `/run/dawos/topology.json`.

#### P0.7 — Campagna di misura

**Cosa**: prove di durata crescente, 10 minuti, 1 ora, 24 ore. Si passa alla successiva solo se la precedente rispetta la soglia.

Misura principale, sui core live, con thread di misura in spazio utente a priorità 95 e arresto automatico con traccia al superamento di 50 µs _(opzioni da verificare con `rtla timerlat hist --help` sulla 6.18)_:

```bash
rtla timerlat hist -c 4-7 -H 20-23 -u -P f:95 -p 250 -d 24h -a 50 \
  --no-header > timerlat-24h.txt
```

Carico, tutto sui domini non live:

```bash
stress-ng --taskset 0-3,8-19 --cpu 12 --vm 4 --vm-bytes 2G --switch 8 --timer 4 --timeout 24h
fio --name=nvme --filename=/data/shared/fio.bin --size=64G --rw=randrw --direct=1 \
  --ioengine=io_uring --iodepth=64 --time_based --runtime=24h
```

Più traffico di rete continuo da un'altra macchina (`iperf3`) e, se disponibile, la Scarlett in riproduzione e registrazione continua con gli strumenti standard di ALSA, per avere il carico di interrupt USB reale.

In parallelo, per tutta la durata: `turbostat` ogni 5 secondi (frequenza, temperatura, potenza, SMI, residenza negli stati di riposo) e i contatori di limitazione termica in `/sys/devices/system/cpu/cpu*/thermal_throttle/`.

Controllo incrociato: una seconda prova di un'ora con `cyclictest` sugli stessi core.

**Accettazione della fase**:

| Grandezza | Soglia | Obiettivo |
| --- | --- | --- |
| Latenza massima di `timerlat`, thread in spazio utente, core live, 24 ore sotto carico | ≤ 50 µs | ≤ 20 µs |
| SMI durante la prova | 0 | 0 |
| Eventi di limitazione termica | 0 | 0 |
| Interrupt di dispositivo sui core live | 0 | 0 |

Il rapporto riporta anche istogramma, 99,999-esimo percentile, e l'impronta esatta di immagine, configurazione del kernel, riga di comando e impostazioni del firmware.

#### P0.8 — Chiusura

Rapporto in `docs/misure/fase-0/rapporto.md`; ADR aggiornati con gli esiti degli esperimenti; elenco di ciò che resta aperto per la Fase 1; etichetta `fase-0`.

### 3.5 Strumenti di misura

| Strumento | Cosa misura | Dove si usa |
| --- | --- | --- |
| `rtla hwnoise` | Rumore di hardware e firmware, con interrupt disattivati | P0.4 |
| `rtla osnoise` | Rumore del sistema operativo su un core: interrupt, softirq, thread | P0.6 |
| `rtla timerlat` | Latenza di risveglio: interrupt del timer, thread, ritorno in spazio utente | P0.7, criterio di fase |
| `cyclictest` | Latenza di risveglio, metodo indipendente | P0.7, controllo incrociato |
| `turbostat` | Frequenza, temperatura, potenza, stati di riposo, contatore SMI | P0.4, P0.7 |
| `trace-cmd`, `perf` | Causa di un superamento | Analisi |
| `stress-ng`, `fio`, `iperf3` | Carico | P0.6, P0.7 |
| `systemd-analyze`, `bootctl` | Tempi e stato dell'avvio | P0.2, P0.5 |

`rtla` è tra gli strumenti del kernel offerti da Buildroot _(verificato)_; `turbostat` non risulta tra quelli elencati e probabilmente richiede un pacchetto nostro in `external/package/` _(da verificare)_.

### 3.6 Limiti noti della Fase 0

- In QEMU si provano le funzioni (avvio, verity, firma, A/B), mai le latenze.
- Il tempo di avvio si misura già, ma l'obiettivo dei 3 secondi si valuta solo con shell e sblocco TPM, cioè nelle fasi 3 e 6.
- LUKS, TPM e homed non si attivano: lo schema del disco li prevede soltanto.
- La soglia di 50 µs riguarda la latenza di scheduling. Non dice ancora nulla sull'audio: quello è il compito della Fase 1.

---

## Parte 4 — Domande aperte

Ogni domanda ha le alternative, i compromessi e la mia raccomandazione. Le prime nove bloccano la Fase 0; le altre vanno chiuse prima della Fase 1 ma non fermano il lavoro sulla piattaforma.

### 4.1 Da decidere prima di iniziare la Fase 0

#### D-01 — Il PC di sviluppo e il 285K sono la stessa macchina?

Non è scritto nel master prompt e cambia il flusso di lavoro.

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| Stessa macchina, due dischi | Nessun hardware in più; da WSL2 si può scrivere direttamente sull'NVMe dedicato (`wsl --mount --bare`). | Ogni prova reale richiede un riavvio; le prove da 24 ore fermano lo sviluppo. |
| Due macchine | Sviluppo e misura in parallelo; SSH e `iperf3` verso il 285K. | Serve un secondo PC. |

**Raccomandazione**: se hai un secondo PC anche modesto, usalo per sviluppare. Altrimenti si procede con una macchina sola e si pianificano le prove lunghe di notte. Dimmi qual è il tuo caso.

#### D-02 — Meccanismo di isolamento dei core nell'MVP

Deriva da C-02 e tocca una decisione vincolante.

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| A. Isolamento statico al boot (`isolcpus`, `nohz_full`, `rcu_nocbs`), affinità per thread, cgroup per processo | Semplice; compatibile con sandbox, limiti di memoria e nftables per servizio; comportamento ben noto. | Cambiare profilo richiede un riavvio; `isolcpus` è considerato in via di sostituzione. |
| B. Partizioni isolate di cgroup v2 fin da subito | Ridimensionabili a runtime. | Thread dello stesso processo in domini diversi richiedono cgroup _threaded_, che tolgono i limiti per processo; `nohz_full` resta comunque al boot. |
| C. Un processo per dominio (motore live e motore anticipativo separati, due host per traccia) | Compatibile con le partizioni. | Raddoppia i processi e complica la promozione al live. |

**Raccomandazione**: A per l'MVP, con l'esperimento del passo P0.6 per sapere se B è praticabile più avanti. Le partizioni dinamiche diventano obiettivo di ricerca.

#### D-03 — Initrd

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| A. Nessun initrd: `dm-mod.create` nella riga di comando firmata | Avvio più rapido; meno componenti; si capisce tutto ciò che accade. | Meno diffuso; l'uso di `PARTLABEL` va verificato; diagnosi più scomoda se qualcosa manca. |
| B. Initrd con systemd | Strada più battuta; partizioni trovate automaticamente dall'impronta. | Circa 0,3–0,5 s in più _(stima)_; più pezzi nell'immagine. |
| C. Initrd minimo nostro | Rapido; didattico. | Codice nostro in un punto critico per la sicurezza. |

**Raccomandazione**: A, con una prova a tempo limitato (un giorno); se non regge, B.

#### D-04 — Schema delle partizioni dei dati

Deriva da C-04. Alternative: (A) tre partizioni `state`, `shared`, `homes` come nella sezione 3.3; (B) un solo volume XFS dentro LUKS, con le immagini di homed dentro, accettando la doppia cifratura; (C) rinunciare a homed e gestire noi un volume LUKS per utente.

**Raccomandazione**: A. Mi serve anche la tua indicazione sulle dimensioni: quanto spazio per le librerie condivise e quanto per i progetti degli utenti. La proposta è 250 GiB e il resto.

#### D-05 — Varianti `dev` e `prod`

**Raccomandazione**: due varianti dallo stesso albero, come descritto in C-05; nella Fase 0 solo `dev`, con SSH a sola chiave. Confermi?

#### D-06 — Libreria C e toolchain

I plugin VST3 sono compilati contro glibc: musl è escluso di fatto. Resta la scelta tra toolchain esterna già pronta (Bootlin, glibc, stabile) e toolchain costruita da Buildroot.

**Raccomandazione**: toolchain esterna Bootlin glibc: la prima costruzione scende da ore a decine di minuti, e la versione è fissata. Si passa alla toolchain interna solo se serve una versione specifica di GCC.

#### D-07 — Versione di Buildroot e politica di aggiornamento

**Raccomandazione**: 2026.02.x LTS fissata come sottomodulo; kernel fissato all'ultima 6.18.y e aggiornato a ogni chiusura di fase, non in mezzo a una campagna di misura. La migrazione alla LTS successiva si valuta quando sarà designata e avrà almeno tre mesi di versioni correttive.

#### D-08 — Licenze

Il repository è MIT. Due punti: il kernel e gli strumenti GPL non creano vincoli sul nostro codice in spazio utente; `dawos-fcpd` invece, se riusasse codice o dati di `fcp-server`, diventerebbe GPL.

**Raccomandazione**: tenere MIT per tutto il codice nostro; scrivere `dawos-fcpd` da zero usando `fcp-server` solo come documentazione del protocollo; in alternativa, nell'MVP, eseguire direttamente `fcp-server` come processo separato e rimandare `dawos-fcpd`. Quale preferisci?

#### D-09 — Stato di riposo dei core live

Deriva da K-04. Polling permanente contro C1, e PM QoS sempre attivo contro attivo solo a motore armato.

**Raccomandazione**: non decidere ora. Il passo P0.6 misura latenza, temperatura e potenza nei due casi; decidiamo sui numeri. La mia aspettativa è che C1 a motore fermo e polling a motore armato sia il compromesso giusto per uno studio.

### 4.2 Da decidere prima della Fase 1

#### D-10 — Modello di passaggio tra motore e host

Alternative: (A) passaggio sincrono sullo stesso core, con assegnazione statica dei segmenti ai core (sezione 2.3); (B) host liberi sui core live a priorità più alta dei worker, lasciando al kernel la scelta del core; (C) plugin esterni caricati dentro il motore per le tracce live, rinunciando all'isolamento.

**Raccomandazione**: A. È deterministico e tiene i buffer in cache; il prezzo è che il bilanciamento lo fa il compilatore del piano e non lo scheduler. C va escluso: un plugin che cade trascinerebbe il motore.

#### D-11 — Formulazione della regola sulle chiamate di sistema

**Raccomandazione**: adottare il testo proposto in C-01, con l'elenco chiuso delle chiamate ammesse (`futex_wake`, `futex_wait`, attesa su ALSA), e farlo verificare da uno strumento: in debug, un filtro seccomp sui thread live che termina il processo a ogni chiamata fuori elenco.

#### D-12 — Codifica del piano di controllo e del journal

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| CBOR | Binario, autodescrittivo, tollera campi nuovi; corrispondenza diretta con JSON per MCP. | Più lento e più grande di un formato a schema fisso. |
| postcard o simili | Compatto e veloce. | Non autodescrittivo: fragile tra versioni, pessimo per un journal che deve durare anni. |
| JSON | Leggibile ovunque. | Lento, ambiguo sui numeri, grande. |

**Raccomandazione**: CBOR per socket e journal, JSON solo come resa per MCP e per gli strumenti di diagnosi; record a layout fisso solo sul piano real-time. I comandi sono rari: la velocità di codifica non conta.

#### D-13 — Contenuto del journal

**Raccomandazione**: comando più modifiche primitive, una transazione per record, come nella sezione 2.6. L'alternativa di registrare solo i comandi e rieseguirli è più compatta ma lega la lettura dei vecchi progetti al codice di allora.

#### D-14 — Se R-01 non regge a 44,1 kHz

Con 128 campioni e due periodi il margine a 44,1 kHz è il più sottile (K-03). Se la misura supera i 10 ms, cosa cede?

**Raccomandazione**: tenere fermi i 10 ms e permettere un quanto di 96 o 64 campioni a 44,1 e 48 kHz come profilo «bassa latenza», con budget di calcolo ridotto. In alternativa si accetta che R-01 valga da 48 kHz in su. Preferisco la prima: la latenza è ciò che il musicista sente.

#### D-15 — Automazione, mappa del tempo e compensazione della latenza nell'MVP

**Raccomandazione**: sì a tutte e tre. Automazione di volume, pan, mute e parametri dei plugin; mappa di tempo e metrica; compensazione della latenza con modalità di monitoraggio a bassa latenza. Non serve che l'interfaccia sia ricca fin da subito, ma comandi, piano e formato del progetto devono prevederle dalla Fase 2. Ti chiedo anche di confermare o escludere le altre voci di M-02 (take e registrazione a ciclo, gruppi e VCA, mandate e sidechain, congelamento).

#### D-16 — Definizione del carico di R-04

**Raccomandazione**: un plugin sintetico di riferimento in tre classi di costo, per esempio 10, 40 e 150 ns per campione su un P-core, in proporzione 60 %, 30 %, 10 % _(numeri da discutere)_; R-04 si riformula come «256 tracce con 8 istanze ciascuna di questa miscela». Così la verifica è ripetibile e non dipende da prodotti di terzi.

#### D-17 — Backend audio alternativi e sviluppo fuori dall'immagine

**Raccomandazione**: sì. Backend `alsa`, `null` e `file` dietro la stessa interfaccia dalla Fase 1; il motore deve compilare e superare i test su una qualunque distribuzione Linux, oltre che nell'immagine. Senza questo, ogni modifica richiederebbe il PC reale.

#### D-18 — Regole per il codice Rust

**Raccomandazione**: versione stabile fissata con `rust-toolchain.toml`; un solo spazio di lavoro in `crates/`; il nucleo real-time in un crate separato, senza allocatore nei percorsi live e con un controllo in debug che interrompe a ogni allocazione; `unsafe` ammesso solo in moduli dedicati, con commento che ne giustifica la correttezza; dipendenze esterne approvate una per una.

#### D-19 — Trasporto MCP nell'MVP

**Raccomandazione**: MCP su socket Unix locale in entrambe le varianti; nella variante `dev` raggiungibile da un'altra macchina tramite tunnel SSH. Nessuna porta di rete in ascolto, mai.

#### D-20 — Rappresentazione del tempo musicale

Alternative: tick interi (proposta: 15 360 per quarto), razionali esatti, virgola mobile in quarti.

**Raccomandazione**: tick interi. Sono esatti, confrontabili, e si scrivono senza perdita in un MIDI Clip File. La virgola mobile accumula errori che emergono proprio nei progetti lunghi.

#### D-21 — Modello di annullamento

Alternative: cronologia lineare unica; cronologie separate per autore; annullamento selettivo di una singola operazione passata.

**Raccomandazione**: lineare unica nell'MVP, con l'autore visibile su ogni voce. Il formato del journal non impedisce di aggiungere l'annullamento selettivo quando arriverà l'agente.

---

## Prossimo passo

Mi fermo qui, come richiesto. Per partire con la Fase 0 mi servono le risposte a D-01…D-09; bastano poche parole per ciascuna, anche solo «come raccomandato». Le altre domande possiamo chiuderle mentre la piattaforma prende forma. Se una contraddizione o un rischio della Parte 1 non ti convince, è il momento di dirlo: preferisco correggere il documento ora che il codice dopo.

## Fonti consultate

Verifiche fatte in questa sessione, il 13 novembre 2026.

- [Linux 6.18 designato kernel a lungo termine](https://linuxiac.com/linux-6-18-is-now-listed-as-the-new-long-term-support-kernel/)
- [Buildroot su Wikipedia: versione stabile 2026.02](https://en.wikipedia.org/wiki/Buildroot)
- [Un anno di Buildroot LTS, FOSDEM 2026](https://fosdem.org/2026/events/attachments/3XV7CP-1y-of-buildroot-lts/slides/267458/br-fosdem_3mareo2.pdf)
- [Serie di patch del driver FCP](https://patchwork.kernel.org/project/alsa-devel/cover/cover.1735159288.git.g@b4.vu)
- [Sorgente `fcp.c` nel kernel 6.18](https://sources.debian.org/src/linux/6.18.9-1/sound/usb/fcp.c)
- [Documentazione del kernel sull'isolamento delle CPU](https://docs.kernel.org/core-api/housekeeping.html)
- [Proposta di gestione dinamica dell'housekeeping, giugno 2026](https://lkml.iu.edu/2606.2/04065.html)
- [Thread dei kernel non vincolati su CPU `nohz_full`, marzo 2026](https://lkml.iu.edu/hypermail/linux/kernel/2603.2/13712.html)
- [Infrastruttura del server `SCHED_DEADLINE`](https://patchew.org/linux/cover.1716811043.git.bristot@kernel.org)
- [Discussione sui parametri del _fair server_, gennaio 2026](https://lkml.iu.edu/2601.1/06016.html)
- [Steinberg adotta la licenza MIT per VST3](https://www.soundonsound.com/news/steinberg-adopt-mit-license-vst3)
- [Licenza di VST 3](https://steinbergmedia.github.io/vst3_dev_portal/pages/VST+3+Licensing/VST3+License)
- [Pagina di manuale di `systemd-stub`](https://www.freedesktop.org/software/systemd/man/latest/sd-stub.html)
- [Pagina di manuale di `ukify`](https://man7.org/linux/man-pages/man1/ukify.1.html)
- [UKI a più profili, FOSDEM 2025](https://fosdem.org/2025/events/attachments/fosdem-2025-5074-multi-profile-ukis-and-other-ways-to-supercharge-your-unified-kernel-images/slides/236693/Multi-Pro_DFoSucB.pdf)
- [Creazione anticipata di dispositivi mappati (`dm-init`)](https://docs.kernel.org/admin-guide/device-mapper/dm-init.html)
- [systemd-homed sul wiki di Arch](https://wiki.archlinux.org/title/systemd-homed)
- [Strumenti del kernel in Buildroot](https://android-kvm.googlesource.com/buildroot/+/refs/heads/next/package/linux-tools/Config.in)
- [Recensione della Scarlett 18i20 di quarta generazione](https://www.soundonsound.com/reviews/focusrite-scarlett-18i20-4th-gen?amp)
- [Linee guida SUSE per la prova di hardware real-time](https://documentation.suse.com/sles/16.1/html/SLES-rt-hardware-testing/)
