# Fase 1 — Audio minimo senza interfaccia: piano dei passi

- **Stato**: proposta del 21 novembre 2026; le decisioni che il passo P1.1 chiedeva sono prese (vedi [le domande](01-domande-da-d10-a-d21.md)); l'ordine dei passi è da confermare con l'utente. È la «Parte 3» del documento 0 per la Fase 1.
- **Origine**: prompt della Fase 1 consegnato dall'utente il 21 novembre 2026; schede «Strato 4 — Audio e MIDI» e «Strato 5 — Motore audio» del documento di progetto, che non sono nel repository; [ADR 0021](../../adr/0021-core-live-senza-kernel.md) e [ADR 0022](../../adr/0022-driver-grafico-all-avvio.md); sotto-passo [P1.0](P1.0-raccordo.md).
- **Documenti che lo accompagnano**: [le domande da D-10 a D-21](01-domande-da-d10-a-d21.md); [il progetto del ciclo a due fasi e della protezione di K-04](P1.1-progetto-ciclo-e-k04.md).

## Obiettivo e prodotti

`dawos-engine`, un processo in Rust comandato da riga di comando, che:

- apre la Scarlett 18i20 con ALSA e porta l'audio dall'ingresso all'uscita;
- misura la propria latenza in loopback;
- esegue un grafo di nodi nativi semplici in parallelo sui core live;
- registra a ogni ciclo il margine rimasto prima della scadenza.

A fine fase si sa, con numeri misurati su audio vero, se la piattaforma della Fase 0 regge il motore.

Prodotti:

| Prodotto | Dove |
| --- | --- |
| Il motore e i suoi crate | `crates/` |
| Il pacchetto che lo mette nell'immagine | `external/package/dawos-engine/` |
| La configurazione del routing della Scarlett per il loopback | `external/board/dawos/x86_64/rootfs-overlay/usr/share/dawos/` |
| Le misure | `docs/misure/fase-1/` |
| Il rapporto di fase | `docs/misure/fase-1/rapporto.md` |

## Che cosa è cambiato da quando il prompt è stato scritto

Il prompt parte dalla chiusura della Fase 0. Tra quella e oggi c'è il sotto-passo P1.0, che ha cambiato quattro delle sue premesse _(verificato, vedi [P1.0](P1.0-raccordo.md))_.

| Il prompt dice | Stato al 21 novembre 2026 | Che cosa propongo |
| --- | --- | --- |
| Il sotto-passo pianificato in P0.8 (A-44, A-48, ricontrolli, misura di conferma) è il primo lavoro | **Fatto**: è P1.0, chiuso con l'etichetta `p1.0` | Il motore si misura sull'immagine `0.0.1.20261120170929` o su quelle che ne derivano |
| La frequenza della parte comune del processore è l'unico candidato per le pause di 18 µs; proporre una prova con la frequenza fissata | **Superato**. La prova è stata fatta e non decide, perché la coda aveva un'altra causa: lo schermo acceso dal firmware e lasciato senza driver grafico. Con il driver caricato ([ADR 0022](../../adr/0022-driver-grafico-all-avvio.md)) le 24 ore danno 6,71 µs di massimo contro 20,70 | Nessuna prova sulla frequenza. Per A-43 restano la prova di regressione da un quarto d'ora e la classificazione dei risvegli sul percorso vero (criterio 4) |
| La Scarlett va collegata alla porta del controller dedicato | **Non esiste una porta così.** Le linee USB 2.0 delle porte Thunderbolt arrivano al controller del chipset, lo stesso di tastiera e schermo touch (punto aperto A-53) | Vedi «Ciò che serve all'utente» |
| La Scarlett 18i20 si porta a un routing noto con `fcp-server` | Vale per la **quarta generazione**. Quella collegata oggi è la **seconda**, che il kernel governa con un altro driver del mixer (`scarlett2`), senza FCP | Vedi la domanda D-28 |
| La macchina certificata non ha moduli caricati | L'immagine carica il driver grafico a ogni avvio: undici moduli | Nessuna azione: è la configurazione misurata nelle 24 ore di P1.0 |

## Ordine dei passi

Un passo non si considera chiuso finché il suo criterio non è stato eseguito e annotato. Ogni passo segue il protocollo del prompt: file completi, scelte spiegate, comandi esatti, versioni e API da verificare, attesa dei risultati.

| Passo | Contenuto | Criteri della fase che prepara o chiude |
| --- | --- | --- |
| P1.0 | Raccordo: immagine nuova, driver grafico, verifica del firmware. **Chiuso.** | — |
| P1.1 | Progetto: ciclo a due fasi, protezione di K-04, domande aperte. Nessun codice. **Chiuso.** | 5, 6 |
| P1.2 | Scheletro: spazio di lavoro Rust, costruzione sul PC e nell'immagine, motore con il backend `null`, riga di comando. **Superato.** | — |
| P1.3 | Il ciclo e la protezione di K-04 con il backend `null`: bilancio di calcolo, sorvegliante, prova d'urto. **Superato**; il margine del bilancio si misura in P1.6 | 6 |
| P1.4 | La scheda: ALSA in `mmap`, ingresso e uscita collegati, due periodi; passthrough; routing della Scarlett. **Superato**, con tre periodi e un minuto di avviamento non giudicato ([esito](P1.4-la-scheda.md)). Con la scheda un thread live solo (A-56); slittamenti all'avvio non spiegati (A-58) | — |
| P1.5 | Calibrazione della latenza in loopback; verifica del segnale campione per campione. Prima domanda, da P1.4: con due periodi corti il segnale salta di mezzo millisecondo ogni mezzo minuto (A-58). **Superato** il 23 novembre 2026 con tre scostamenti decisi dall'utente: tre periodi da 96 campioni (D-14), ripetibilità dentro un avvio (A-59), tre avvii su cento a 192 kHz con uno slittamento di avvio ([esito](P1.5-latenza-in-loopback.md#parte-4-lesito)). R-01: 5,5–7,8 ms per il cavo a 44,1, 48, 96 e 192 kHz | 1 |
| P1.6 | La misura a ogni ciclo: margine, ritardo di risveglio, syscall e ingressi nel kernel, cause degli xrun. **Diviso in quattro sottopassi** il 23 novembre 2026 ([guida](P1.6-la-misura-a-ogni-ciclo.md)): P1.6a il registro dei cicli (A-61), P1.6b tutti i thread live sulla scheda (A-56, A-55), P1.6c chi entra nel kernel (K-22), P1.6d far rispettare la regola sulle syscall (dal 24 novembre), P1.6e margine (A-54) e accettazione. **P1.6a superato** il 23 novembre 2026: nessun record perso, costo non misurabile, A-61 chiuso (è la scheda), nuovo A-62 (flussi che partono corti dopo un cambio di frequenza). **P1.6b superato** il 23 novembre 2026: gli altri thread live dormono sul timer del flusso (A-56 chiuso; `poll()` sul flusso provata e scartata) e cominciano il ciclo entro un microsecondo dal primo; risveglio 7,7 µs medi e 14,5 al massimo con quattro thread (A-55 resta aperto). **P1.6c, prima misura** il 23 novembre 2026: la traccia del kernel sui core live letta in binario e messa nelle fasi del ciclo (`--kernel-entries`); in 390 311 cicli zero syscall e zero ingressi nel kernel nel calcolo, due syscall per ciclo fuori dal calcolo; **seconda misura** lo stesso giorno: A-55 chiuso (il thread dell'interrupt USB tiene il lock del flusso mentre sveglia i thread in fila; accettato), `kernel.timer_migration` a 0 nel profilo (le due mode sul timer erano i timer spostati su un altro core), nuovi A-63, A-64 e A-65. **P1.6c superato** il 24 novembre 2026: con la regolazione nell'immagine, sul timer nessun risveglio oltre i 6,2 µs a giro avviato; zero syscall e zero ingressi nel kernel nel calcolo in 2 400 654 cicli. **P1.6c chiuso** (`p1.6c`). **P1.6d consegnato** il 24 novembre 2026: il kernel ferma le syscall del calcolo (_Syscall User Dispatch_), il motore le nomina e le esegue al posto di chi le ha fatte. **P1.6d superato** lo stesso giorno: 24 025 syscall fatte apposta nel calcolo, tutte fermate ed eseguite senza xrun né slittamenti; costo non misurabile quando nessuno chiama il kernel, 1,2 µs per chiamata fermata. **P1.6d chiuso** (`p1.6d`). **P1.6e consegnato** il 24 novembre 2026: la serie sul margine con la scheda e due ore di accettazione (scelta dell'utente: una di solo passthrough e una al 60 %) sotto il carico della campagna. **Misurato il 24 novembre 2026**: le due ore superate (14,4 milioni di cicli, nessun xrun, nessuno slittamento, nessuna syscall nel calcolo, risveglio peggiore 15,8 µs); il margine resta 150 µs (A-54 chiuso: la scheda non ne impone uno); limite dichiarato di 20 µs per il tempo perso senza ingresso nel kernel. **P1.6e superato**, e con questo l'intero passo P1.6: le chiamate di funzione dagli altri core viste nel calcolo sono `do_sync_core`, la sincronizzazione per la riscrittura del codice del kernel (A-49) | 4, 5 |
| P1.7 | Il grafo: compilazione, scambio atomico, un thread per core live, nodi nativi, carico sintetico. **In sette sottopassi** dal 25 novembre 2026 (erano sei: lo scambio e il nodo abbandonato sono due consegne) ([guida](P1.7-il-grafo.md)): P1.7a il grafo fuori dal tempo reale (superato), P1.7b il bilancio nuovo e lo strumento (A-68; superato il 25 novembre: all'85 % di carico nessun ciclo tagliato dopo uno scatto), P1.7c l'esecuzione nel ciclo (superato il 25 novembre: il conteggio passato tra quattro thread senza un campione fuori posto, e il ciclo vero entro mezzo microsecondo dal programma del compilatore), P1.7d il carico e le misure (superato il 25 novembre: il carico è filtraggio vero su AVX2, classi entro il 3 % a 96 kHz, denormali settanta volte più cari senza FTZ e DAZ, ciclo misurato entro il 3 % dalla carta e accelerazione 3,64 su quattro thread; aperto A-71), P1.7e lo scambio (superato il 25 novembre: stato per nome del nodo, chiesto dalla riga di comando), P1.7f il nodo abbandonato (superato il 25 novembre: un grafo senza il nodo in uso 16 ms dopo il blocco), P1.7g l'accettazione (**superato il 26 novembre 2026: criterio 7 raggiunto**; classi del carico entro lo 0,5 %, accelerazione 3,56 su quattro thread a 96 kHz, un'ora di scambi sotto il carico della campagna senza un campione fuori posto; aperti A-73, A-74, A-75 e A-76). Decisioni dello stesso giorno: assegnazione statica dei nodi ai core (D-10, [ADR 0025](../../adr/0025-grafo-assegnazione-statica.md)), formato di testo nostro, bilancio contato dall'inizio vero del ciclo. **Da P1.12**: il bilancio di calcolo deve reggere lo scatto di un millisecondo dell'annuncio del periodo, e lo strumento deve contarlo a parte (A-68) | 7 |
| P1.8 | Il driver audio (Q-14, A-50): le configurazioni misurate con il motore; decisione su `dawos-uac`. Da P1.5: i gradini del giro tra un avvio e l'altro (A-59) e il pavimento di 5,4 ms (A-60). **Da P1.12** (25 novembre 2026): sulla 18i20 di quarta generazione il driver di serie aggiunge 3,5 ms al giro, e R-01 non è rispettato a 44,1 e 48 kHz (A-69); il periodo arriva a blocchi di un millisecondo (A-68). **Da P1.7e**: una scrittura corta all'avvio dei flussi (A-72). **In sottopassi** dal 26 novembre 2026 ([guida](P1.8-il-driver-audio.md)): P1.8a, il bilancio della latenza sulla quarta generazione (scelta dell'utente), consegnato lo stesso giorno: il sorgente dice che il driver dimensiona i trasferimenti USB sul periodo chiesto, e si misura il giro con un periodo più corto del ciclo; i sottopassi seguenti si decidono con l'esito. **Esito di P1.8a** (26 novembre): quattro periodi del driver in un ciclo tolgono 0,5 ms a tutte le frequenze (9,73 ms per il cavo a 48 kHz, 10,43 a 44,1); con otto il flusso si rompe. P1.8b, leggere il bus USB con `usbmon` (scelta dell'utente), **misurato lo stesso giorno**: dei 3,5 ms sotto il buffer, 3,0 sono il tempo che l'uscita passa presso il controller USB (2,0–2,5 di anticipo voluto da `xhci-hcd`, più la durata del trasferimento); il resto è la costante della seconda generazione. Un driver audio nostro non li toccherebbe. P1.8c (scelta dell'utente): la prova di una patch di una riga a `snd-usb-audio`, dietro un interruttore, che consegna l'uscita «appena possibile»; **misurato lo stesso giorno a macchina ferma**: giro per il cavo a 8,35 ms a 48 kHz e 8,95 a 44,1 con `96:3` (erano 10,23 e 10,93), R-01 rispettato a tutte le frequenze, nessun difetto in ottanta avvii. P1.8d (scelta dell'utente): la stessa forma, quattro thread, un'ora sotto il carico della campagna con l'interruttore acceso, poi i cento avvii di R-01; **misurato lo stesso giorno**: l'ora `PASS` (giro 729 campioni all'inizio e alla fine, nessuno slittamento, nessun xrun), i cento avvii tutti sotto i 10 ms (al massimo 9,09 ms a 44,1 kHz, 8,35 a 48, 7,85 a 96, 7,80 a 192). **R-01 rispettato sulla quarta generazione.** **Decisioni dell'utente** ([ADR 0026](../../adr/0026-driver-audio-di-serie-con-iso-asap.md)): la patch si adotta dalla riga di comando del profilo `285k`; `dawos-uac` non si scrive in Fase 1. P1.8e: l'adozione nell'immagine e il margine della consegna, letto sul bus sotto carico; **misurato il 27 novembre**: la macchina parte con `usb_asap=on`; sotto carico la consegna più tarda di 238 000 ha consumato 45 µs dei circa 115 disponibili, nessun microframe saltato. P1.8f: l'avvio dei flussi (A-72, A-59, A-62), poi la chiusura | 1, 8 |
| P1.9 | Il kernel (Q-13, A-49): stima e piano sottoposti all'utente, poi la prova della serie sul rinvio degli interrupt tra core. **Da P1.7g** (26 novembre 2026): le pulizie della TLB che il processo provoca sui propri processori live (A-75) e la pulizia del predittore a ogni programma BPF classico caricato (A-76) | 8 |
| P1.10 | La campagna: 24 ore a 96 kHz, un'ora a 48 e una a 192 kHz, prova d'urto finale | 2, 3, 4, 5, 6 |
| P1.11 | Chiusura: rapporto, ADR, elenco degli aperti per la Fase 2 | 9 |
| P1.12 | **Si fa tra P1.6 e P1.7** (il numero è in coda per non rinumerare). La Scarlett 18i20 di quarta generazione: `fcp-support` e firmware nell'immagine ([ADR 0024](../../adr/0024-interfaccia-fcp-server-e-firmware.md)), stato noto del routing, di nuovo la latenza di P1.5 e le due ore di P1.6e. In tre sottopassi ([guida](P1.12-la-scarlett-di-quarta-generazione.md)): P1.12a **superato** il 24 novembre 2026 (firmware da 1.4.2301.0 a 3.0.2778.0, 959 controlli, il motore apre la scheda alle tre frequenze); P1.12b, i file dello stato noto, **superato** lo stesso giorno (prova breve 8 casi su 8, nessun xrun e nessuno slittamento); P1.12c **superato** il 25 novembre con due scostamenti decisi dall'utente: due ore senza xrun né slittamenti, con l'ora al 60 % giudicata sul fenomeno (A-68); **R-01 non rispettato a 44,1 e 48 kHz** con `96:3` e il driver di serie (10,9 e 10,2 ms, A-69), rinviato a P1.8. La quarta generazione è l'interfaccia del resto della fase | 1, 2 |

Perché quest'ordine:

- **K-04 prima dell'audio** (P1.3 prima di P1.4), come chiede il prompt: la protezione si prova con un ciclo scandito da un timer, senza scheda. Un motore che parla con la scheda nasce già protetto.
- **La latenza presto** (P1.5): R-01 è il rischio K-03, e se a due periodi non regge cambia il progetto di tutto ciò che segue.
- **Gli strumenti di misura prima del grafo** (P1.6 prima di P1.7): il grafo si giudica con il margine per ciclo, che deve esistere prima.
- **Driver e kernel dopo il grafo** (P1.8, P1.9): il prompt chiede che le misure che decidono siano fatte «sul ciclo vero».

## I passi

### P1.1 — Progetto del ciclo e della protezione

**Cosa**: il documento [P1.1](P1.1-progetto-ciclo-e-k04.md), e le risposte dell'utente alle domande che toccano la fase.

**Accettazione**: l'utente conferma, o corregge, il modello del ciclo, i tempi della protezione e le decisioni D-11, D-14, D-17, D-18, D-26, D-27 e D-28. Le conferme entrano negli ADR.

### P1.2 — Scheletro del motore

**Cosa**:

- `rust-toolchain.toml` con una versione stabile fissata; spazio di lavoro in `crates/`;
- crate proposti: `dawos-rt` (il nucleo real-time: ciclo, code senza lock, orologio, nessun allocatore nei percorsi live), `dawos-audio-io` (i backend `null`, `file` e `alsa` dietro una sola interfaccia), `dawos-graph` (da P1.7), `dawos-engine` (il processo e la riga di comando);
- costruzione e prove sul PC di sviluppo, senza l'immagine; `clippy` senza avvisi dentro `make lint`;
- pacchetto Buildroot che compila il motore con la toolchain dell'immagine e lo installa; dipendenze copiate nel repository o scaricate con somma di controllo, senza rete durante la costruzione dell'immagine;
- `dawos-engine run --backend null`: un ciclo scandito da un timer, su un core live, che non calcola nulla.

**Da verificare**: versione di Rust e supporto di Buildroot 2026.02 ai pacchetti `cargo`; che il binario parta sull'immagine (glibc della toolchain Bootlin, [ADR 0006](../../adr/0006-toolchain-bootlin-glibc.md)).

**Accettazione**: `cargo test` e `cargo clippy` puliti sul PC; `make image` produce un'immagine con `dawos-engine`; sul 285K `dawos-engine run --backend null` gira un minuto su un core live e stampa il numero dei cicli.

### P1.3 — Il ciclo e la protezione di K-04

**Cosa**: il ciclo a due fasi con il bilancio di calcolo; il sorvegliante fuori dai core live; i thread di riserva; la prova d'urto come comando del motore (chiude A-07); il punto A-06 riletto alla luce del progetto.

**Accettazione**: sul 285K, con il backend `null` a 2,667 ms di periodo:

- un nodo che non termina e un thread live che non cede vengono rilevati e isolati nei tempi fissati in P1.1;
- il motore continua a girare sugli altri core e segnala l'evento;
- i thread del kernel legati a quel core avanzano (contatori letti prima e dopo);
- nessun riavvio, nessuna unità fallita.

### P1.4 — La scheda

**Cosa**: backend `alsa`; apertura in `mmap` del dispositivo `hw`, ingresso e uscita collegati (`snd_pcm_link`), due periodi da un quanto; frequenze da 44,1 a 192 kHz con il quanto che dura sempre 2,667 ms (128, 256 e 512 campioni); passthrough dall'ingresso all'uscita; il routing della Scarlett portato a uno stato noto da un file nel repository; ripresa dopo un xrun e dopo lo scollegamento dell'interfaccia.

**Da verificare**: versione di alsa-lib nell'immagine; che cosa `snd-usb-audio` della 6.18.64 accetta come periodo e buffer a ogni frequenza (lo dice `aplay --dump-hw-params`); il comportamento di `snd_pcm_link` con questo driver; lo strumento per il routing secondo la generazione dell'interfaccia.

**Accettazione**: passthrough per dieci minuti a 48, 96 e 192 kHz senza xrun, con un segnale in ingresso che si ritrova in uscita; i parametri accettati dal driver scritti nelle misure.

### P1.5 — Latenza in loopback

**Cosa**: calibrazione con un impulso e con una sequenza nota; latenza di andata e ritorno in campioni e in millisecondi; verifica del segnale campione per campione, per trovare i difetti che ALSA non segnala (rischio K-08).

**Accettazione**: criterio 1 della fase. Cento calibrazioni a 48, 96 e 192 kHz, due periodi: latenza sotto i 10 ms a ogni frequenza, ripetibilità entro un campione. In più, non richiesto dal criterio ma necessario a D-14: la stessa misura a 44,1 kHz.

**Scostamenti decisi il 22 novembre 2026**, sulle misure: tre periodi da 96 campioni invece di due da 128 (due non reggono); la ripetibilità entro un campione vale dentro un avvio dei flussi, e tra gli avvii si riporta la distribuzione (A-59). Da questo passo il quanto predefinito del motore è 96, 192 e 384 campioni e i periodi sono tre: dove i passi precedenti scrivono 128 campioni e 2,667 ms, oggi sono 96 e 2 ms.

### P1.6 — La misura a ogni ciclo

**Cosa**: timestamp e margine di ogni ciclo in un buffer circolare senza lock, letto da un thread fuori dai core live; istogrammi; ogni xrun contato con la sua causa; il ritardo di risveglio sul percorso vero; le tre misure dell'ADR 0021 (syscall per fase del ciclo, ingressi nel kernel non volontari, tempo perso senza ingresso nel kernel).

**Accettazione**: un'ora di passthrough a 96 kHz sotto la miscela di carico della campagna; il rapporto riporta le distribuzioni; ogni ingresso nel kernel durante il calcolo è contato e ha un nome.

**Sottopassi**, decisi con l'utente il 23 novembre 2026 e diventati cinque il 24: P1.6a, il registro dei cicli; P1.6b, tutti i thread live sulla scheda; P1.6c, chi entra nel kernel; P1.6d, far rispettare la regola sulle syscall (_Syscall User Dispatch_); P1.6e, il margine e l'accettazione qui sopra. I criteri di ciascuno sono nella [guida](P1.6-la-misura-a-ogni-ciclo.md).

### P1.7 — Il grafo

**Cosa**: descrizione del grafo in un file; compilazione fuori dal tempo reale; scambio atomico al confine di un buffer; contatori di dipendenze atomici, un thread per core live; i nodi nativi (ingresso e uscita, guadagno, somma, bus, linea di ritardo, sinusoide, impulso, rumore, misuratore di picco); il nodo di carico sintetico con la miscela di R-04 (10, 40 e 150 ns per campione al 60, 30 e 10 %); virgola mobile a 32 bit, somma a 64 bit sui bus, FTZ e DAZ, AVX2 e FMA.

**Accettazione**: criterio 7. Lo stesso grafo su 1, 2, 3 e 4 core live, con accelerazione e percorso critico misurati; lo scambio di grafo durante il passthrough senza discontinuità nel segnale; il nodo di carico calibrato contro `dawos-dspbench` ([ADR 0011](../../adr/0011-r04-carico-sintetico.md)).

### P1.8 — Il driver audio

**Cosa**: le configurazioni di `snd-usb-audio` lette nei sorgenti (sezione 10.3 del [rapporto della Fase 0](../../misure/fase-0/rapporto.md)), misurate con il motore: latenza reale, jitter del periodo, syscall per ciclo con e senza notifica del puntatore.

**Accettazione**: criterio 8, per Q-14. Se una configurazione rispetta insieme R-01 e l'ADR 0021, `dawos-uac` non si scrive, ed è scritto. Altrimenti il progetto del driver passa prima dall'utente.

### P1.9 — Il kernel

**Cosa**: prima la stima del backport della serie sul rinvio degli interrupt tra core e il piano della prova, sottoposti all'utente; poi, con il suo consenso, la prova sul ciclo vero. Prima ancora, una traccia che dica che cosa sono le chiamate di funzione che oggi arrivano ai core live sotto carico.

**Accettazione**: criterio 8, per Q-13. L'esito della prova scritto; l'adozione non blocca la fase.

### P1.10 — La campagna

**Cosa**: 24 ore a 96 kHz e 256 campioni, grafo sintetico al 60 % del dominio live, più il carico di sistema della campagna P0.7 e il traffico di rete; un'ora a 48 kHz e una a 192 kHz; la prova d'urto di K-04 ripetuta sul motore completo. Da P1.7g: il verdetto della campagna guarda che il carico giri, non quanto traffico di rete c'è stato (4,5 GB in un'ora, 95 nella successiva, tutte e due «PASS»); prima delle 24 ore gli va data una soglia. Da P1.8d: se la patch `iso_asap` è adottata, le 24 ore si fanno con l'interruttore acceso, e sono la sua conferma; il giudizio delle ore chiede già il giro alla fine uguale a quello dell'inizio e nessun campione fuori posto. Da P1.8e: un microframe saltato sul bus non cambia il giro finché la memoria dell'interfaccia lo assorbe, ma lascia il tempo dell'uscita presso il controller più lungo di 125 µs: le ore devono leggere il bus per due secondi all'inizio e alla fine, e contare la differenza.

**Accettazione**: criteri 2, 3, 4, 5 e 6. Zero xrun; margine minimo e distribuzione riportati; ritardo di risveglio sotto i 50 µs con i casi sopra i 20 µs classificati; zero syscall nel calcolo e al massimo due nell'attesa.

### P1.11 — Chiusura

**Cosa**: `docs/misure/fase-1/rapporto.md`; ADR aggiornati; punti aperti per la Fase 2.

**Accettazione**: criterio 9; etichetta `fase-1`.

## Rischi nuovi del registro K

Il registro arriva a K-16. Aggiungo questi.

| Id | Rischio | Gravità | Quando si scopre |
| --- | --- | --- | --- |
| K-17 | Il risveglio del motore attraversa due core: l'interrupt USB arriva sul dominio io e sveglia un thread su un core live. La Fase 0 ha misurato il risveglio da un timer dello stesso core (2 µs in media): questo percorso è più lungo e non è misurato | Alta | P1.4, P1.6 |
| K-18 | `snd-usb-audio` in modalità a bassa latenza chiede una chiamata al kernel per notificare il puntatore a ogni ciclo, e può far lavorare il thread del motore al posto dell'interrupt: tensione con l'ADR 0021 | Alta | P1.8 |
| K-19 | La Scarlett condivide il controller USB con tastiera, schermo touch e LED (A-53): l'isolamento previsto per K-07 non c'è | Media | P1.4, con una misura dedicata |
| K-20 | Il motore si sviluppa sulla 18i20 di seconda generazione e si accetta sulla quarta: firmware, latenza interna e controllo del mixer sono diversi | Media | Quando arriva la quarta generazione |
| K-21 | Costruire Rust dentro l'immagine: versione del compilatore, dipendenze senza rete, collegamento con la glibc della toolchain | Media | P1.2 |
| K-22 | Misurare disturba: contatori e tracce per i criteri 4 e 5 possono aggiungere proprio gli ingressi nel kernel che devono contare | Media | P1.6 |
| K-23 | I worker in attesa attiva dentro il ciclo tengono i core live sempre svegli quando il grafo è leggero: calore, e nessun riposo in C1 durante il calcolo | Bassa | P1.7 |
| K-24 | La coda dei 20 µs torna se il driver grafico cambia comportamento con un kernel nuovo: il meccanismo non è noto ([ADR 0022](../../adr/0022-driver-grafico-all-avvio.md)) | Media | A ogni cambio di kernel o firmware: prova di regressione |

K-04, K-07 e K-08 restano come sono nel documento 0; i loro rimedi sono nei passi P1.3, P1.4 e P1.5.

## Ciò che serve all'utente

### Il cavo di loopback

Un cavo **jack da 6,35 mm bilanciato (TRS) da entrambi i lati**, corto, da un'uscita di linea a un ingresso di linea della Scarlett.

- **Da quale uscita**: una uscita di linea posteriore che non sia governata dalla manopola del volume dei monitor. Propongo la **3**.
- **A quale ingresso**: un ingresso posteriore usato a livello di linea, con il guadagno al minimo e senza alimentazione phantom né altre funzioni accese. Propongo il **3**.
- _(Da verificare sul pannello dell'interfaccia: numerazione, quali uscite segue la manopola dei monitor, se gli ingressi posteriori sono combinati XLR e jack. Lo ricordo per la seconda generazione, non l'ho verificato.)_

Facoltativo, e utile: un **cavo coassiale RCA** dall'uscita S/PDIF all'ingresso S/PDIF. Misura l'andata e ritorno senza i convertitori, e per differenza dice quanta latenza è dei convertitori e quanta dei buffer.

### La porta

La porta Thunderbolt 4 numero 1 del pannello posteriore, dov'è adesso, con il cavo USB-B/USB-C. Non è un controller dedicato (A-53), ma è la configurazione misurata in P1.0: 5,63 µs e nessuna interruzione del flusso.

Finché il punto A-53 non è misurato, durante le misure che contano non si usano tastiera e schermo touch.

Se la misura di A-53 dirà che il controller condiviso disturba, la via più pulita che vedo è una scheda USB su PCIe nello slot lasciato libero dalla scheda grafica: un controller con il suo interrupt, tutto per la Scarlett. È una spesa, e una decisione dell'utente, da prendere con i dati.

### La Scarlett di quarta generazione

Quando arriva: il criterio di accettazione della fase nomina la 18i20, e il prompt nomina `fcp-server`, che è della quarta. Vedi la domanda D-28.

**È arrivata il 24 novembre 2026.** Scelta dell'utente dello stesso giorno: ci si passa **subito dopo la chiusura di P1.6, prima del grafo (P1.7)**, con un passo a sé: `fcp-server` nell'immagine, il routing noto, e di nuovo le misure di latenza (P1.5) e le due ore (P1.6e). I punti aperti da A-57 ad A-62 sono stranezze viste sulla seconda generazione, ed è meglio sapere se valgono anche per la quarta prima di progettare il grafo e il lavoro sul driver. Scartati: aspettare la fine della fase (si scoprirebbe tardi un comportamento diverso) e tenerle collegate tutte e due (due interfacce sullo stesso controller USB, punto aperto A-53).

Il passo è il **P1.12** ([guida](P1.12-la-scarlett-di-quarta-generazione.md)), superato il 25 novembre 2026. **Scelta dell'utente dello stesso giorno: la quarta generazione è l'interfaccia del resto della fase**; la seconda si ricollega nel passo P1.8 per il confronto tra i due modi del driver. La forma del buffer resta `96:3`: sulla quarta dà 10,2 ms a 48 kHz e 10,9 a 44,1, sopra il limite di R-01, perché il driver aggiunge 3,5 ms che nessuna forma tocca (A-69). Scelte dell'utente del 24 novembre 2026: il firmware dell'interfaccia sta nell'immagine, l'interfaccia si aggiorna prima di misurarla, e il passo prende un numero nuovo in coda invece di rinumerare gli altri.
