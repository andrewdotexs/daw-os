# Rapporto della Fase 0

- **Che cos'è**: il rapporto di chiusura della Fase 0, chiesto dal documento 0 al passo P0.8. Dice che cosa è stato misurato, su quale configurazione, con quale esito, e che cosa resta aperto per la Fase 1.
- **Macchina**: il 285K (ASUS ROG STRIX Z890-I, Intel Core Ultra 9 285K, 64 GB); la scheda completa è in [hardware-285k.md](../../fasi/fase-0/hardware-285k.md).
- **Date**: la fase è andata dal 13 al 19 novembre 2026; la campagna di misura dal 18 al 19.
- **Stato**: **in completamento**. Mancano le letture e i conteggi della sezione 10.4 e l'aggiornamento degli ADR; il rapporto diventa definitivo alla chiusura del passo P0.8.

Le etichette sono quelle del documento 0: _(verificato)_ per ciò che è misurato o letto nei dati, _(stima)_ per ciò che è calcolato da altri numeri, _(da verificare)_ per ciò che è supposto.

## 1. Esito

**La Fase 0 rispetta i quattro criteri di accettazione del documento 0.**

| Criterio | Soglia | Misurato | Esito |
| --- | --- | --- | --- |
| Latenza massima di `timerlat`, thread in spazio utente, core live, 24 ore sotto carico | 50 µs | 20,70 µs registrati; 21–22 µs stimati (sezione 5) | Rispettato |
| SMI durante la prova | 0 | 0 | Rispettato |
| Eventi di limitazione termica | 0 | 0 | Rispettato |
| Interrupt di dispositivo sui core live | 0 | 0 | Rispettato |

**L'obiettivo dei 20 µs non è rispettato**, di circa uno o due microsecondi. Per regola fissata dall'utente il 18 novembre 2026, prima di conoscere il risultato, l'obiettivo non decide il verdetto della fase: diventa il punto aperto A-43 della Fase 1.

In una riga: su 1,38 miliardi di risvegli in 24 ore, il ritardo medio è 2,03 µs, il 99,999-esimo percentile 5,75 µs, e sei risvegli hanno superato i 20 µs.

## 2. La configurazione misurata

Dichiarata ferma il 17 novembre 2026, alla chiusura del sotto-passo P0.6e, e non più toccata: nessuna immagine nuova, nessuna modifica al firmware, nessun modulo caricato.

| Che cosa | Valore | Impronta |
| --- | --- | --- |
| Immagine | `0.0.1.20261117231225`, variante `dev`, dal commit `4cdf08c` | Radice verificata con dm-verity: `368ee91656306b6f246b9c0bf6198ccdbba5a366a567fe6c547c2d3c52f3eddd` |
| Kernel | Linux 6.18.64, `PREEMPT_RT`, costruito il 17 novembre 2026 | Configurazione (`/proc/config.gz`), SHA-256: `6e7f79ed1b7a5262f85a03db44657816671b98974c4d136da06c9cd6248ff642` |
| Riga di comando | Profilo `285k` (testo completo sotto la tabella) | SHA-256: `f40878c883a3bcfadf299d32f450c974b1133500520ec807f853e040b47ec60a` |
| Firmware | 3402 del 18 settembre 2026; microcodice `0x122` | Voci registrate in `docs/misure/fase-0/firmware-3402.txt`, SHA-256 del file: `7fedfc8b74a5b2eeae6576f8289bea9b57f3f727061399952e71a856766bfa23` |
| Avvio | Secure Boot attivo, radice verificata, nessuna unità fallita | Rapporto di avvio: `secureboot=1 verity=1 failed_units=0` |
| Strumento della campagna | `tools/campaign.sh` del commit `264d5d0` | SHA-256: `c88ac8d588e87badc8db98302b6cd53c9e459a8cdc72b2094b59fec9b0d67f05` |
| Strumenti di misura | `rtla` 6.18.63, `cyclictest` 2.80, `stress-ng` 0.20.00, `fio` 3.41 | Sono quelli dell'immagine |

La parte della riga di comando che riguarda il tempo reale:

```text
skew_tick=1 tsc=reliable isolcpus=domain,managed_irq,4-7 nohz_full=2-7 rcu_nocbs=2-7 irqaffinity=20-23 dawos.profile=285k
```

Le impostazioni del firmware che contano per la misura _(verificato in P0.6c e P0.6e)_: _Intel Default Settings: Performance_; _Enhanced C-states: Disabled_; limiti di potenza PL1 = PL2 = 160 W.

Che cosa fa il profilo `285k` dopo l'avvio ([ADR 0018](../../adr/0018-profili-di-avvio-e-regolazione.md)):

| Dominio | Core | Regolazione |
| --- | --- | --- |
| Live | 4–7 (P-core) | Isolati; frequenza fissa a 4,6 GHz; riposo limitato a C1, senza attesa attiva; _fair server_ spento |
| Anticipativo | 0–3 (P-core) e 8–15 (E-core) | P-core a frequenza fissa al massimo, 5,5 GHz nominali |
| Sistema | 16–19 (E-core) | Servizi del sistema |
| Io | 20–23 (E-core) | Tutti gli interrupt di dispositivo |

Nessun modulo del kernel è caricato: né il driver grafico `xe`, né i moduli per la macchina virtuale.

## 3. Come è stato misurato

Il dettaglio è in [P0.7-campagna-di-misura.md](../../fasi/fase-0/P0.7-campagna-di-misura.md).

- **La misura**: `rtla timerlat hist` con un thread in spazio utente per core live, `SCHED_FIFO` a priorità 95, periodo di 250 µs, per 24 ore. Istogramma a passi di 250 ns fino a 100 µs.
- **Il carico**, mai sui core live, a cicli di 30 minuti ripetuti 48 volte: 13 minuti con la miscela del documento 0 (`stress-ng` con calcolo, memoria, cambi di contesto e timer sui core 0–3 e 8–19, `fio` sui core 20–23); 10 minuti di puro calcolo su tutti e venti i core non live, alla potenza limite; 5 minuti di raffiche brevi sui P-core non live, in tre forme; 2 minuti di riposo.
- **Il disco**: `fio` su un file di 64 GiB nella partizione `shared`, letture casuali senza limite e scritture a 25 MiB/s con _flush_ ogni 16 MiB.
- **La rete**: due flussi SSH continui dal PC di sviluppo, uno per verso.
- **Il monitoraggio**, mai sui core live: `turbostat` ogni 5 secondi sugli altri venti core; contatori termici, collegamento di rete e interrupt di dispositivo dei core live una volta al minuto; contatore degli SMI di tutti i core prima e dopo.
- **Prima delle 24 ore**, come chiede il documento 0: 10 minuti e 1 ora di `timerlat`, e 1 ora di controprova con `cyclictest`, ciascuna partita solo dopo che la precedente aveva rispettato la soglia.

Scostamenti dal documento 0, tutti nel [registro delle modifiche](../../01-registro-modifiche-v1.md):

| Il documento 0 chiedeva | È stato fatto | Perché |
| --- | --- | --- |
| Carico costante per 24 ore | Carico a cicli, con una fase a 160 W in più | Per misurare anche le raffiche brevi (A-33) e la piena potenza (A-30); la miscela del documento 0 da sola porta il package a 120 W |
| `fio` senza limiti | Scritture a 25 MiB/s | Usura dell'SSD: scelta dell'utente |
| Traffico di rete con `iperf3` | Flussi SSH | `iperf3` non è nell'immagine ferma |
| La Scarlett in riproduzione e registrazione, «se disponibile» | Non fatto | I programmi ALSA non sono nell'immagine ferma: punto aperto A-41 |
| Arresto con traccia al superamento di 50 µs | Traccia sopra i 20 µs, e la misura prosegue | Per sapere che cosa succede quando l'obiettivo è superato: scelta dell'utente |

## 4. Risultati delle 24 ore

Dal 18 novembre 2026, 13:00 UTC, al 19 novembre, 13:00 UTC. Dati in `docs/misure/fase-0/raw/campaign-20261118T130002Z`.

| Grandezza | Valore |
| --- | --- |
| Campioni, thread in spazio utente | 1 382 299 704 |
| Ritardo medio | 2,03 µs |
| 99,999-esimo percentile | 5,75 µs |
| Ritardo massimo registrato | 20,70 µs |
| Campioni oltre i 100 µs dell'istogramma | 0 |
| Potenza del package, media e massima | 111,6 e 160,1 W |
| Temperatura massima del package | 85 °C (limite del processore: 105 °C) |

Per core live, in microsecondi. «All'interrupt» è il ritardo con cui parte il gestore del timer; «thread» quello con cui il thread di misura viene risvegliato nel kernel; «spazio utente» quello con cui torna a eseguire il proprio codice.

| Core | Medio in spazio utente | Massimo all'interrupt | Massimo del thread | Massimo in spazio utente |
| --- | --- | --- | --- | --- |
| 4 | 2,01 | 17,80 | 20,18 | 20,19 |
| 5 | 2,01 | 10,85 | 19,00 | 19,39 |
| 6 | 2,10 | 18,47 | 20,90 | 20,70 |
| 7 | 2,01 | 12,79 | 19,58 | 19,99 |

### L'istogramma

Campioni per microsecondo di ritardo, sommati sui quattro core. L'ultima colonna conta i risvegli in spazio utente con quel ritardo o uno maggiore. L'istogramma completo, a passi di 250 ns e per core, è nel file `1-timerlat-86400/timerlat.txt` della cartella dei dati.

| Ritardo (µs) | All'interrupt | Thread | Spazio utente | Spazio utente, da qui in su |
| --- | --- | --- | --- | --- |
| 0–1 | 1381937527 | 0 | 0 | 1382299704 |
| 1–2 | 309164 | 1379343829 | 583250014 | 1382299704 |
| 2–3 | 42432 | 2551713 | 797986288 | 799049690 |
| 3–4 | 8359 | 344188 | 935615 | 1063402 |
| 4–5 | 1602 | 44917 | 101123 | 127787 |
| 5–6 | 323 | 9499 | 18146 | 26664 |
| 6–7 | 129 | 2936 | 4735 | 8518 |
| 7–8 | 39 | 1231 | 1814 | 3783 |
| 8–9 | 19 | 600 | 875 | 1969 |
| 9–10 | 5 | 318 | 442 | 1094 |
| 10–11 | 10 | 178 | 258 | 652 |
| 11–12 | 6 | 96 | 141 | 394 |
| 12–13 | 9 | 65 | 85 | 253 |
| 13–14 | 2 | 44 | 52 | 168 |
| 14–15 | 3 | 34 | 42 | 116 |
| 15–16 | 1 | 11 | 21 | 74 |
| 16–17 | 1 | 17 | 17 | 53 |
| 17–18 | 3 | 13 | 16 | 36 |
| 18–19 | 1 | 10 | 10 | 20 |
| 19–20 | 0 | 4 | 8 | 10 |
| 20–21 | 0 | 4 | 2 | 2 |

Letto in parole: 999 risvegli su 1000 stanno sotto i 3 µs; uno su centomila supera i 5,75 µs; circa uno su un milione supera i 9 µs; 253 in tutto superano i 12 µs.

### Per fase del ciclo

Il ritardo massimo di ogni fase, letto dal tracciatore in microsecondi interi; 48 fasi per tipo.

| Fase | Che cosa gira | Ore in tutto | Minimo | Mediana | Massimo | Potenza media | Temperatura massima |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `full` | Miscela del documento 0 | 10,4 | 12 | 17 | 20 | 122,7 W | 82 °C |
| `power` | Puro calcolo su venti core | 8,0 | 5 | 5 | 6 | 160,0 W | 85 °C |
| `burst-a` | Raffiche di 1 ms sui P-core non live | 1,3 | 5 | 6 | 9 | 28,6 W | 54 °C |
| `burst-b` | Raffiche di 5 ms | 1,3 | 5 | 6 | 12 | 33,3 W | 55 °C |
| `burst-c` | Timer e cambi di contesto a qualche kHz | 1,3 | 5 | 6 | 9 | 19,1 W | 42 °C |
| `rest` | Niente | 1,6 | 5 | 5 | 6 | 8,6 W | 38 °C |

- **La coda sta tutta nella miscela del documento 0.** Alla potenza limite, con il solo calcolo, il massimo è 6 µs in otto ore.
- **Le raffiche brevi sui P-core vicini non disturbano più i core live**: era il problema scoperto in P0.6d (ritardi fino a 55 µs), e il rimedio regge con tre carichi diversi da quello che l'aveva fatto scoprire.
- **La temperatura non segue l'ora del giorno** oltre due o tre gradi: nelle fasi `power` 82 °C di notte, 83–85 °C di giorno.

### Il resto

- **Sui core live sono arrivati solo interrupt del kernel**: il timer della misura (1 382 494 862), 3817 lavori differiti dovuti alla lettura del massimo per fase, 9202 chiamate di funzione, 4 pulizie della TLB. Le chiamate di funzione sono circa una al minuto per core, oltre a quelle dell'avvio e dell'arresto dello strumento; a macchina ferma non ce ne sono (punto aperto A-47).
- **Disco**: letture a 6,0 GiB/s e scritture a 25,0 MiB/s per tutta la durata.
- **Rete**: 2,1 TiB per verso, tra 16 e 34 MiB/s per verso. **Sette cadute del collegamento**, da 3,6 a 7,1 secondi, tutte nelle prime due ore e mezza (punto aperto A-36).
- **Registro del kernel**: solo le righe delle cadute del collegamento.
- **Buchi nella misura**: 6,3 secondi su 24 ore, circa un secondo per ciascuna delle sei tracce salvate.

## 5. I sei risvegli sopra i 20 µs

Ogni risveglio sopra i 20 µs ha lasciato una traccia del kernel. Sono sei, e hanno tutti la stessa forma _(verificato sulle tracce)_.

| Traccia | Ora (UTC) | Core | All'interrupt | Durata del gestore del timer | Thread | Spazio utente |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 18 nov, 16:31:15 | 4 | 7,4 | 11,4 | 19,2 | 20,2 |
| 2 | 18 nov, 16:31:17 | 6 | 18,5 | 1,3 | 20,7 | non registrato |
| 3 | 18 nov, 18:00:56 | 6 | 0,5 | 18,6 | 19,9 | 20,7 |
| 4 | 19 nov, 03:01:28 | 4 | 17,8 | 2,3 | 20,2 | non registrato |
| 5 | 19 nov, 07:31:34 | 6 | 1,2 | 16,5 | 20,4 | non registrato |
| 6 | 19 nov, 12:32:20 | 6 | 0,6 | 19,8 | 20,9 | non registrato |

- **Quanta parte del ritardo è già all'ingresso dell'interrupt**: quasi tutta in due eventi (17,8 e 18,5 µs su 20); in altri tre il ritardo è dentro il gestore del timer, che dura 16–20 µs invece di 1; in uno è a cavallo dei due. In tutti e sei spariscono tra 16,5 e 19,8 µs, sempre circa la stessa quantità.
- **In che fase del ciclo**: tutti nella fase `full`, tra 26 e 56 secondi dal suo inizio, su 780 secondi di durata.
- **Che cosa girava sul core live**: niente. Il core era a riposo; nella traccia non ci sono altri interrupt, né NMI, né altri thread.
- **Su più core insieme**: nella traccia 3, nello stesso istante, il core 4 è in ritardo di 14,9 µs e il core 6 di 20,7 µs.

**Il massimo vero in spazio utente è circa 21–22 µs** _(stima)_. In quattro eventi la traccia si è fermata quando il ritardo del thread ha passato i 20 µs, prima che lo strumento registrasse il ritorno in spazio utente; negli altri due quel passaggio aggiunge 0,8–1,0 µs. Il valore registrato, 20,70 µs, è quindi di poco in difetto. Sulla soglia dei 50 µs non cambia nulla.

**Che cosa sono, come ipotesi**: pause di circa 18 µs che vengono da sotto il sistema operativo e toccano più core insieme, provocate da qualcosa che la miscela di carico fa nel suo primo minuto. Non sono SMI (contatore fermo), né limitazioni termiche (contatori fermi), né effetto del limite di potenza (a 160 W il massimo è 6 µs). Dalla coda dell'istogramma le pause sarebbero dell'ordine del migliaio in 24 ore _(stima)_: quasi sempre cadono tra un risveglio e l'altro e non si vedono. La causa non è nota; trovarla è il punto aperto A-43.

## 6. Le misure brevi e la controprova

Il 18 novembre 2026, prima delle 24 ore, con la sola miscela del documento 0 e cicli di dieci minuti. Dati in `docs/misure/fase-0/raw/campaign-20261118T100850Z`.

| Passo | Campioni | Medio | 99,999 % | Massimo | Esito |
| --- | --- | --- | --- | --- | --- |
| `timerlat`, 10 minuti, periodo 250 µs | 9 600 003 | 2,05 | 6,50 | 18,62 | Passato |
| `timerlat`, 1 ora, periodo 250 µs | 57 599 992 | 2,10 | 6,75 | 20,66 | Passato |
| `cyclictest`, 1 ora, periodo 1 ms | 14 399 993 | 1,00 | 12 | 19 | Passato |

In tutti e tre: nessun SMI, nessuna limitazione termica, nessun interrupt di dispositivo sui core live.

I due strumenti concordano sul massimo. Sul 99,999-esimo percentile no: 12 µs per `cyclictest`, 6,75 µs per `timerlat`. Cambiano insieme lo strumento e il periodo, e non è stato misurato quale dei due conti _(da verificare, A-43)_. Conta per la Fase 1: il motore audio lavorerà con periodi più vicini al millisecondo che ai 250 µs.

## 7. Gli esperimenti della fase

| Esperimento | Domanda | Esito | Dove |
| --- | --- | --- | --- |
| E-01 | Attesa attiva, C1 o C1E per i core live a riposo | C1, senza attesa attiva; C1E spento nel firmware | [ADR 0009](../../adr/0009-riposo-dei-core-live.md) |
| E-02 | _Fair server_ acceso o spento sui core live | Spento: il suo timer raggiungeva ogni core live una volta al secondo. In cambio serve una protezione contro un thread real-time che non cede mai il core (A-18) | [ADR 0018](../../adr/0018-profili-di-avvio-e-regolazione.md) |
| E-03 | I gruppi di controllo possono fare da confine per i core isolati? | L'isolamento statico all'avvio è confermato; l'esperimento ne ha corretto le motivazioni | [ADR 0002](../../adr/0002-isolamento-statico-dei-core.md) |
| E-04 | Quale driver grafico, e quanto rumore porta | `i915` non si compila con `PREEMPT_RT`; resta `xe`, sperimentale per questo processore, come modulo non caricato. Con lo schermo fermo non porta rumore; il carico grafico vero non è misurato (A-23, rischio K-16) | [P0.6c](../../fasi/fase-0/P0.6c-cgroup-e-driver-grafico.md) |
| E-05 | Lo streaming regge dentro una home cifrata su loop? | Sì, con due vincoli: cifratura senza code di lavoro, e diario del progetto su segmenti già scritti | [ADR 0004](../../adr/0004-partizioni-dei-dati.md) |
| E-06 | Quanto vale un E-core rispetto a un P-core su carico DSP | Tra 0,51 e 0,59; il dominio anticipativo vale 8,7–9,4 core live; soglia di utilizzo del dominio portata al 75 % | [ADR 0011](../../adr/0011-r04-carico-sintetico.md) |

Due fatti scoperti lungo la strada, che hanno cambiato la configurazione:

- **I P-core non live vanno tenuti a frequenza fissa.** Quando lavoravano a raffiche brevi e a frequenza bassa, un core live a riposo si risvegliava fino a 55 µs in ritardo. Con la frequenza fissa al massimo: 6 µs, confermati dalla campagna. Costa qualche watt a riposo (da 3 a 7–8 W).
- **Caricare un modulo del kernel interrompe tutti i core**, quelli live compresi: circa 4200 interrupt brevi per core per i moduli della macchina virtuale. I moduli si caricano prima che l'audio parta, mai durante.

## 8. Numeri di riferimento della configurazione

Dall'accettazione del sotto-passo P0.6e, sull'immagine misurata, dieci minuti per misura; e dagli esperimenti.

| Grandezza | Valore |
| --- | --- |
| Rumore hardware su tutti i core, dieci minuti | Nessun buco di almeno 1 µs, nessun SMI |
| Rumore sui core live a 160 W, dieci minuti | Massimo 7 µs |
| Ritardo di risveglio, macchina ferma | 1,8 µs medio, 4,4 µs massimo |
| Ritardo di risveglio, altri core a 160 W | 1,8 µs medio, 13,4 µs massimo |
| Potenza del package a riposo | 7–8 W |
| Temperatura a 160 W | 80–85 °C |
| Capacità del dominio anticipativo | 8,7–9,4 core live equivalenti; la sessione di riferimento a 96 kHz ne usa 6,08 |
| Disco | Letture a 7,1 GB/s; _flush_ in 4,8 ms |
| Tempo di avvio | 20 secondi, di cui 14 del firmware |

## 9. Limiti di questa misura

- **Una macchina, una corsa di 24 ore.** Il risultato vale per questo esemplare, questo firmware, questa immagine.
- **Nessun carico audio reale.** La scheda audio USB non ha lavorato durante la campagna (A-41): gli interrupt USB reali, sul dominio io, non sono stati misurati.
- **Nessun carico grafico.** Il driver grafico non era caricato (A-23).
- **I core live erano a riposo tra un risveglio e l'altro.** È il caso peggiore per il ritardo di risveglio, ma non è il motore audio: quello arriverà con la Fase 1.
- **La causa delle pause di 18 µs non è nota**, e con essa non è noto se un carico diverso ne produrrebbe di più lunghe.
- **Il massimo in spazio utente è stimato per quattro eventi su sei** (sezione 5).
- **Il verdetto delle 24 ore è stato riscritto una volta.** Alla fine della corsa lo strumento della campagna non ha saputo leggere l'istogramma, completo, e ha scritto «nessun campione»: `rtla`, con conteggi a nove cifre, scrive la riga dei conteggi in una forma che lo strumento non riconosceva. Lo strumento è stato corretto e il verdetto rifatto dagli stessi file, senza rimisurare; il primo verdetto è conservato in `result-first.txt`.
- **Il periodo di misura è 250 µs**, quello del documento 0; con 1 ms il 99,999-esimo percentile potrebbe essere più alto (sezione 6).

## 10. Dopo la campagna: letture e decisioni del 19 novembre 2026

Sulla configurazione ferma, senza installare né modificare nulla.

### 10.1 Il rumore hardware a macchina calda (A-40)

`dawos-hwnoise`, dieci minuti, il 19 novembre alle 13:30 UTC, mezz'ora dopo la fine delle 24 ore. Dati in `docs/misure/fase-0/raw/hwnoise-20261119T133014Z-dopo-campagna`.

| Grandezza | Questa misura | Accettazione di P0.6e (17 novembre) | Misure precedenti |
| --- | --- | --- | --- |
| Buchi di almeno 1 µs, SMI | Nessuno, 0 | Nessuno, 0 | Nessuno, 0 |
| Potenza media del package | 93,1 W | 94,6 W | 92–97 W |
| Temperatura massima | 66 °C | 51 °C | 65–66 °C |
| Letture in dieci minuti | 56 | 120 | 52–55, nelle due misure da dieci minuti del 17 novembre |
| Occupazione dei core live | 61 % | 75 % | 60–68 % |

**La norma è 65–66 °C**; la misura dell'accettazione di P0.6e è l'eccezione, in tutte e tre le grandezze insieme. **La spiegazione che avevo proposto, il kernel 6.18.64, è smentita**: questa misura è fatta con lo stesso kernel. Di diverso, quella del 17 novembre aveva l'essere stata fatta cinque minuti dopo un avvio. Si ricontrolla alla prossima immagine (A-44).

### 10.2 Le decisioni dell'utente e i numeri assegnati

L'utente ha valutato se riscrivere firmware, kernel e driver; la risposta è no per tutti e tre, con un principio comune al loro posto. Nel repository:

| Repository | Documento di progetto | Che cosa |
| --- | --- | --- |
| [ADR 0021](../../adr/0021-core-live-senza-kernel.md) | ADR-038 | Core live senza kernel: nella fase di calcolo i thread del motore non entrano nel kernel e non vengono interrotti |
| A-48 | Q-12 | Verifica del firmware all'avvio, al posto di un port di coreboot |
| A-49 | Q-13 | Prova di una serie di patch che rinvia gli interrupt tra core |
| A-50 | Q-14 | Un driver audio proprio solo se la misura lo richiede |

La tabella di corrispondenza tra le due numerazioni è in [docs/adr/README.md](../../adr/README.md).

**Obiezioni all'ADR 0021**: nessuna al principio. Tre integrazioni, proposte prima di scriverlo e accolte dall'utente:

1. **Una terza misura**: il tempo perso nel calcolo senza ingresso nel kernel. Le pause di 18 µs della sezione 5 non sono né syscall né interrupt, e le due misure previste non le vedrebbero.
2. **Le fasi valgono per ogni thread**, e dentro il calcolo nessun thread ne sveglia un altro: il modo più ovvio di coordinare quattro core live è proprio una syscall nel calcolo.
3. **Un bilancio di calcolo**: l'attesa cede il core ai thread del kernel solo se il motore dorme davvero. In sforamento non dorme; il motore smette quindi di calcolare a un margine fisso prima della fine del periodo, e il sorvegliante esterno resta come seconda linea.

Per contare e attribuire le syscall nel calcolo l'ADR propone, oltre al contatore per thread, il meccanismo _Syscall User Dispatch_ del kernel, che dice quale plugin ha chiamato il kernel senza costare nulla finché nessuno lo fa _(da verificare sul 285K, Fase 1)_.

### 10.3 Come si comporta `snd-usb-audio` con il puntatore dell'applicazione

Letto nel sorgente del kernel 6.18.64 (`sound/usb/pcm.c`, `endpoint.c`, `card.c`; `sound/core/pcm_native.c`) _(verificato nel sorgente; non provato sulla macchina)_.

- **L'opzione `lowlatency` del driver è attiva di fabbrica.** Quando è attiva, **ogni** flusso di riproduzione dichiara al nucleo ALSA che il puntatore dell'applicazione va sincronizzato in modo esplicito (`SNDRV_PCM_INFO_SYNC_APPLPTR`), e il nucleo rifiuta allora di mappare in memoria la pagina di controllo. L'applicazione non può scrivere `appl_ptr` in memoria: deve passare dall'ioctl `SYNC_PTR`, **una syscall per ciclo**. La pagina di stato, con `hw_ptr`, resta leggibile in memoria. La registrazione non è toccata: lì entrambe le pagine sono in memoria.
- **In modalità a bassa latenza** il driver riempie gli URB in uscita solo con i dati fino a `appl_ptr`. Finché ci sono URB in volo li riempie al completamento del precedente, dal gestore dell'interrupt USB. Quando non ce ne sono più, è la notifica dell'applicazione a riempirli e a spedirli (`snd_usb_pcm_playback_ack`): **l'invio USB avviene allora dentro la syscall dell'applicazione, cioè sul core live**.
- **Senza notifica, in modalità a bassa latenza, il driver non riempie nulla.** Non c'è un ripiego automatico alla coda di URB: quello è il comportamento della modalità classica.
- **La modalità classica** vale in tre casi: con `lowlatency=0`; per un dispositivo con feedback implicito; per un flusso che non si ferma mai sugli errori (`stop_threshold` oltre il buffer). Gli URB si riempiono al completamento dei precedenti, dall'interrupt, con ciò che c'è nel buffer, e la latenza è quella della coda di URB.
- **Solo con `lowlatency=0` il puntatore si scrive in memoria**, senza syscall. Un dispositivo con feedback implicito lavora in modalità classica ma, con l'opzione attiva, paga comunque la syscall.

La risposta al dubbio della nota è quindi sì, con una precisazione: i casi da misurare nella Fase 1 sono **tre**, non due.

| Configurazione | Syscall per il puntatore | Dove si riempiono gli URB | Latenza |
| --- | --- | --- | --- |
| `lowlatency` attiva, bassa latenza | Una per ciclo | Interrupt USB; a coda vuota nella syscall, sul core live | La più bassa |
| `lowlatency` attiva, feedback implicito | Una per ciclo | Interrupt USB | Coda di URB |
| `lowlatency=0` | Nessuna | Interrupt USB | Coda di URB |

Se la Scarlett di quarta generazione sia trattata con feedback implicito non risulta dal sorgente: per Focusrite non c'è una regola dedicata al feedback, e il riconoscimento generico dipende dai descrittori che il dispositivo presenta _(da verificare con la scheda collegata: `/proc/asound/card*/stream0`)_.

### 10.4 I dati in sola lettura chiesti per A-49 e A-50

Ciò che il repository sapeva già _(verificato)_:

| Domanda | Risposta | Da dove |
| --- | --- | --- |
| Il kernel ha l'isolamento delle tabelle delle pagine? | Sì, compilato (`MITIGATION_PAGE_TABLE_ISOLATION`); `pti=on` si può provare senza ricostruire il kernel | Configurazione del kernel |
| Pulizie della TLB sui core live sotto carico | 4 in 24 ore, sui quattro core insieme | Campagna |
| Chiamate di funzione sui core live sotto carico | Circa una al minuto per core, oltre a quelle dello strumento di misura; mittente non noto | Campagna, A-47 |
| Moduli NVIDIA o `nouveau` | Nessun modulo caricato, di nessun tipo; `nouveau` non è nel kernel | Campagna, configurazione |
| Driver della RTX 5080 | Nessuno. La sua funzione audio HDMI è però presa da `snd_hda_intel`, che è nel kernel | [hardware-285k.md](../../fasi/fase-0/hardware-285k.md) |
| Controller USB | Due: quello del processore (Thunderbolt 4, `00:0d.0`), oggi vuoto, e quello del chipset (`80:14.0`), con tastiera e ricevitori | Topologia del 17 novembre |
| Dove è collegata la Scarlett | Non era collegata il 17 novembre: nessuna scheda audio vista dal kernel | Topologia del 17 novembre |

Ciò che si legge ora, con `tools/read-after-campaign.sh`, in `docs/misure/fase-0/letture-dopo-campagna.txt`: stato di Meltdown e delle altre vulnerabilità; i registri del processore che descrivono il firmware (riferimento per A-48); controller, interrupt e affinità dell'USB; la scheda audio, se collegata; avvisi di blocco del bus e frequenza della parte comune del processore (A-43). **In attesa della lettura.**

Ciò che si conta ora, con il piano `ipi` della campagna, tre ore: gli interrupt arrivati a ciascun core live in un'ora a macchina ferma, in un'ora con il solo monitoraggio della campagna, in un'ora sotto carico, senza alcuno strumento di tracciamento. Risponde alla domanda di A-49 e separa, per A-47, ciò che manda lo script da ciò che manda il carico. **In attesa della misura.**

## 11. Che cosa resta aperto per la Fase 1

L'elenco completo, con l'origine di ogni punto, è il [registro dei punti aperti](../../02-punti-aperti.md). Qui in ordine di quando serve.

**Prima di cominciare la Fase 1**:

| Punto | Che cosa |
| --- | --- |
| A-44 | Caricare i moduli KVM e vhost all'avvio, e rifare le misure brevi sull'immagine nuova ([ADR 0019](../../adr/0019-apertura-del-sistema-utilita-native-e-vm-linux.md)) |
| A-48 | La verifica del firmware a ogni avvio, nello stesso sotto-passo ([ADR 0021](../../adr/0021-core-live-senza-kernel.md)) |
| A-18, A-06, A-07 | La protezione contro un thread real-time che non cede mai il core, e la prova che la verifica |
| A-41 | La misura con la scheda audio USB che suona e registra |
| A-36 | Le cadute del collegamento di rete: dentro o fuori dalla macchina |
| A-40 | La misura a 51 °C fatta cinque minuti dopo un avvio: un fatto dei primi minuti, o una misura isolata |
| A-19, A-20 | Le decisioni rinviate: D-10…D-15, D-17…D-21, D-23, D-24, D-25 |
| A-21 | La sorveglianza del blocco del sistema |
| A-22 | Gli stati di riposo profondi, assenti per tutti i core |
| A-32 | I buffer delle istanze sfalsati dentro la pagina: requisito per l'allocatore del motore |

**Durante la Fase 1**:

| Punto | Che cosa |
| --- | --- |
| A-43 | L'obiettivo dei 20 µs: la causa delle pause di 18 µs; strumento o periodo nel confronto tra `timerlat` e `cyclictest` |
| A-47 | Chi manda una chiamata di funzione al minuto a ogni core live durante la campagna |
| A-49 | La prova della serie di patch che rinvia gli interrupt tra core, con un piano da confermare prima di scrivere codice |
| A-50 | La misura che decide se serve un driver audio proprio: latenza di andata e ritorno, syscall per ciclo, tre configurazioni di `snd-usb-audio` |

**Più avanti**:

| Punto | Che cosa |
| --- | --- |
| A-45 | La macchina virtuale: dominio `vm` sui core 18–19, e ciò che resta da precisare |
| A-46 | I plugin di terze parti: dominio dei loro thread, corpus di riferimento ([ADR 0020](../../adr/0020-plugin-linux-di-terze-parti.md)) |
| A-23, A-29 | Il carico grafico vero; lo stato del driver `xe` |
| A-37, A-38, A-39 | Configurazione del boot loader negli aggiornamenti; riavvio nel firmware; registri degli errori hardware senza interrupt |
| A-24, A-25, A-26 | Profilo delle ventole; revisione legale della licenza; seconda macchina supportata |

## 12. Dove sono i dati

| Che cosa | Dove |
| --- | --- |
| Le 24 ore | `docs/misure/fase-0/raw/campaign-20261118T130002Z` |
| 10 minuti, 1 ora, controprova | `docs/misure/fase-0/raw/campaign-20261118T100850Z` |
| Le due prove di cinque minuti | `docs/misure/fase-0/raw/campaign-20261118T095643Z`, `campaign-20261118T124501Z` |
| L'accettazione della configurazione ferma | `docs/misure/fase-0/raw/`, cartelle `hwnoise-`, `osnoise-` e `wakeup-` che finiscono in `-ferma` |
| Topologia e flag del processore | `docs/misure/fase-0/topologia-285k.txt` |
| Le guide dei passi, con scelte e risultati | `docs/fasi/fase-0/` |

In ogni cartella di una campagna: `meta.txt` (impostazioni e impronte), `summary.txt`, e per ogni passo l'istogramma, il massimo per fase (`phases.txt`), le letture di `turbostat`, i campioni al minuto, i contatori prima e dopo, le tracce.
