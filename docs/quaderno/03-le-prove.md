# 3. Le prove e come leggerle

Ogni strumento che compare nelle guide dei passi, con il significato di ciò che stampa. Le uscite sono in inglese, come il codice; qui sono spiegate riga per riga.

## Dove gira che cosa

Una prova passa per tre luoghi, sempre in quest'ordine:

1. **il PC di sviluppo**: le prove automatiche del codice e i controlli di forma;
2. **la macchina virtuale** sul PC di sviluppo: l'immagine intera, per vedere che parta e che la logica funzioni;
3. **il 285K**: le misure che contano.

Se una prova fallisce in un luogo non si passa al successivo.

| Comando | Dove | Che cosa fa |
| --- | --- | --- |
| `make engine-test` | PC | Le prove automatiche del motore (vedi sotto) |
| `make lint` | PC | I controlli di forma: `rustfmt`, `clippy`, `shellcheck` sugli script, `markdownlint` sui documenti, e il controllo che nessun dato privato sia finito nel repository |
| `make image` | PC | Costruisce l'immagine del sistema |
| `make qemu-test` | PC, macchina virtuale | Avvia l'immagine e legge il rapporto d'avvio |
| `make qemu-audio-test` | PC, macchina virtuale | Avvia l'immagine e le fa eseguire la prova audio sulla scheda di software |
| `make remote-update HOST=INDIRIZZO REBOOT=1` | dal PC al 285K | Manda l'immagine al 285K, che la scrive nello slot libero e riavvia |
| `make remote-test HOST=INDIRIZZO PROFILE=285k EXPECT=…` | dal PC al 285K | Legge il rapporto d'avvio del 285K e lo confronta con ciò che ci si aspetta |
| `ssh root@INDIRIZZO …` | sul 285K | Lancia uno strumento di misura |
| `make measure-fetch HOST=INDIRIZZO` | dal 285K al PC | Copia i file delle misure in `docs/misure/fase-0/raw/`, dopo aver controllato che non contengano dati privati |
| `tools/close-step.sh SIGLA COMMIT TITOLO NOTA` | PC | Chiude un passo: lo unisce al ramo principale e gli mette un'etichetta |

## Le prove automatiche

`make engine-test` stampa quattro righe `test result`, una per ognuno dei pezzi (_crate_) in cui è diviso il codice Rust. I numeri che compaiono nelle guide, per esempio «8, 77, 33 e 37», sono le prove passate in ognuno:

| Crate | Che cosa contiene |
| --- | --- |
| `dawos-audio-io` | Le sorgenti dei periodi: un timer (`null`) e la scheda (`alsa`) |
| `dawos-engine` | Il processo: il ciclo, la protezione, il registro, il rapporto, la riga di comando |
| `dawos-graph` | Il grafo: la lettura della descrizione, la compilazione, l'esecuzione |
| `dawos-rt` | Il nucleo: orologio, priorità, statistiche senza allocazioni, i filtri, e l'unico codice `unsafe` |

Queste prove giudicano **la logica, non i tempi**: girano su qualunque macchina, anche occupata. Una prova che chiedeva «nove cicli su dieci interi» falliva su un PC carico ed è stata riscritta (capitolo 5).

## Il rapporto d'avvio

A ogni avvio il sistema scrive una riga che riassume in che stato è. `make qemu-test` e `make remote-test` la leggono.

```text
DAWOS-BOOT-REPORT version=0.0.1.20261125143131 variant=dev kernel=6.18.64-dawos realtime=1 secureboot=1 verity=1 rootpart=… profile=285k tune=ok gpu=xe firmware=same kvm=1 timers=none var=state failed_units=0 uptime_s=32.00
```

| Campo | Che cosa dice |
| --- | --- |
| `version` | La versione dell'immagine: contiene data e ora della costruzione |
| `variant` | `dev` è la variante di sviluppo, con accesso remoto e strumenti di misura |
| `kernel` | La versione del kernel |
| `realtime=1` | Il kernel è PREEMPT_RT |
| `secureboot=1` | Il firmware ha verificato la firma di ciò che ha avviato |
| `verity=1` | La radice è verificata blocco per blocco |
| `rootpart` | La partizione da cui si è avviato: dice quale dei due slot è in uso |
| `profile` | Il profilo di avvio: `285k` sul 285K, `generic` nella macchina virtuale di `qemu-test` |
| `tune` | Le regolazioni del profilo che si fanno dopo l'avvio (`dawos-tune`): `ok` applicate per intero; `off` con il profilo `generic`, che non ha nulla da applicare; `degraded` se il profilo non va bene per questa macchina o una regolazione non è riuscita; `none` se manca il rapporto |
| `gpu` | Il driver grafico in uso: `xe` sul 285K |
| `firmware` | Il confronto tra le impostazioni del firmware e quelle registrate nel profilo: `same`, `changed`, oppure `skipped`, `no-reference`, `unreadable`, `none` quando il confronto non si è potuto fare. Una differenza è una nota, non un avvio fallito |
| `kvm=1` | La virtualizzazione è disponibile |
| `timers` | I timer di systemd attivi: `none`, oppure i loro nomi. Deve essere `none`: un timer è un lavoro che parte da solo a un'ora che nessuno ha scelto (capitolo 5, il riavvio delle 4:10) |
| `var` | Dove sta lo stato che sopravvive ai riavvii: `state` è la partizione apposta, `tmpfs` vuol dire che è in memoria e andrà perso |
| `failed_units=0` | Nessun servizio di sistema è fallito |
| `uptime_s` | Secondi dall'accensione al rapporto |

`make remote-test` confronta questi campi con ciò che gli si dice di aspettarsi: `EXPECT="gpu=xe kvm=1 firmware=same"`.

## Gli strumenti della Fase 0

Sono gli strumenti con cui si è misurata la piattaforma prima che esistesse il motore. Servono ancora: a ogni immagine nuova si ricontrolla che la macchina sia quella di prima.

### La latenza di schedulazione

**`rtla timerlat`** è lo strumento del kernel per la domanda «un timer scade: quanto dopo riparte il thread che lo aspettava?». Per ogni processore dà tre ritardi, in microsecondi, ognuno con `cur` (l'ultimo), `min`, `avg`, `max`:

| Colonna | Fin dove misura |
| --- | --- |
| `IRQ` | Fino al gestore dell'interruzione del timer: il processore è uscito dal riposo ed è entrato nel kernel |
| `Thread` (`Thr`) | Fino al thread che aspettava, ancora dentro il kernel: in più c'è il cambio di contesto |
| `Ret user` (`Usr`) | Fino al ritorno nel programma: è ciò che vedrebbe il motore, ed è il numero del criterio dei 50 µs |

**`cyclictest`** misura la stessa cosa con un metodo indipendente (un thread che dorme fino a un istante e guarda di quanto ha sforato); conta la colonna `Max`. Due strumenti perché uno solo può avere un difetto suo: nella campagna, infatti, non concordano sul 99,999-esimo percentile (12 µs contro 6,75), e il perché non è chiuso.

**I percentili.** «99,999-esimo percentile 5,75 µs» vuol dire che un risveglio su centomila è stato più lento di 5,75 µs. A 500 cicli al secondo, uno su centomila è uno ogni tre minuti e venti: per questo si guarda quello, e il massimo, più della media.

### Il rumore dell'hardware

**`dawos-hwnoise`** (sotto c'è `rtla hwnoise`). Su ogni processore un thread del kernel, con le interruzioni spente, legge l'orologio di continuo: un «buco» tra due letture è tempo che **l'hardware o il firmware** hanno rubato, perché nient'altro poteva interromperlo. Risoluzione 1 µs; misura 750 ms ogni secondo.

| Riga | Che cosa dice |
| --- | --- |
| `max_single_noise_us` | Il buco più lungo. Sul 285K: 0 |
| `smi_count` | Gli SMI contati dal processore durante la misura. Deve essere 0 |
| `thermal`, `thermal_throttle_events` | Temperatura del processore e interventi della protezione termica |
| `verdict` | `PASS` con buchi fino a 15 µs e nessun SMI |

### Il rumore del sistema

**`dawos-osnoise`**. Lo stesso principio, ma con le interruzioni accese e sui soli processori live, mentre `stress-ng` carica tutti gli altri: ciò che interrompe il thread di misura è **il sistema operativo**. Conta le interruzioni per tipo, con le sigle di `/proc/interrupts`:

| Sigla | Che cos'è |
| --- | --- |
| `LOC` | Il timer locale del processore (il _tick_) |
| `RES` | Un altro processore chiede di rifare la schedulazione |
| `CAL` | Un altro processore chiede di eseguire una funzione (_IPI_) |
| `TLB` | Un altro processore chiede di svuotare la cache delle traduzioni degli indirizzi |
| `IWI` | Lavoro differito chiesto dal kernel a sé stesso |
| `MCP` | Il controllo periodico degli errori hardware |

Il verdetto è `FAIL` per qualunque interruzione che non sia il timer locale. È con questo strumento che si è trovato il _fair server_ (P0.6a).

### Il costo del risveglio

**`dawos-wakeup`** misura quanto costa a un processore live svegliarsi dai diversi stati di riposo. Sono giri di `rtla timerlat` a 1 ms in cinque condizioni:

| Caso | Condizione |
| --- | --- |
| `rest` | Nulla gira e nessun timer è armato: potenza e temperatura della macchina a riposo |
| `idle-quiet`, `idle-load` | I processori live si fermano tra una scadenza e l'altra, con il resto della macchina fermo o sotto carico |
| `busy-quiet`, `busy-load` | Un ciclo a vuoto a bassa priorità tiene svegli i processori live: il timer non li trova mai a riposo. Con il resto fermo o sotto carico |

Le colonne `irq`, `thr`, `usr` sono i tre ritardi di `timerlat`. La differenza tra `idle` e `busy` è il **prezzo del risveglio** (righe `wakeup_cost_…`); `staying_awake_cost_…` è ciò che costa in watt non dormire.

### Temperatura, potenza e frequenza

**`turbostat`** legge i contatori del processore: temperatura, potenza del package, frequenza effettiva, SMI.

**`dawos-thermal-sweep`**: carico continuo con limiti di potenza crescenti, cinque minuti per gradino.

| Colonna | Che cosa dice |
| --- | --- |
| `limit_w`, `measured_w` | La potenza concessa al processore e quella che ha usato davvero |
| `temp_end_c` | La temperatura alla fine del gradino |
| `drift_c_per_min` | Di quanto stava ancora salendo: vicino a zero vuol dire che il gradino è a regime |
| `busy_mhz` | La frequenza a cui giravano i processori carichi |
| `smi` | SMI nel gradino |

**`dawos-freq-sweep`**: fissa i processori live a una frequenza e guarda se la tengono. `holds: yes` se ognuno resta entro l'1 % del valore chiesto.

### Le prove del sistema

| Comando | Che cosa prova |
| --- | --- |
| `make qemu-tamper-test` | Si cambia un byte nell'immagine (nella radice, nell'albero delle impronte o nel file di avvio): il sistema **non deve partire** |
| `make qemu-ab-test` | Un'immagine che fallisce l'avvio tre volte: il caricatore deve tornare da solo a quella di prima |
| `make qemu-update-test` | Un aggiornamento firmato viene installato; uno manomesso viene rifiutato |
| `dawos-cgroup-test` | Dodici domande al kernel sui gruppi di controllo che tengono ogni processo nel suo dominio; ogni riga finisce con `as expected` |
| `dawos-gpu-test` | Il driver grafico lavora (carico, cambio di immagine sullo schermo) senza disturbare i processori live |
| `dawos-starve-test` | Un thread real-time che non cede mai per 30 secondi: quanto aspettano un processo normale e un lavoro del kernel sullo stesso processore. È la misura del rischio K-04 |

### Il calcolo e il disco

**`dawos-dspbench`**: un nucleo di filtri in C, vettoriale, misurato su ogni tipo di processore. Le righe `ecore/live` e `pcore/live` dicono quanto vale un E-core e un P-core dell'altro dominio rispetto a un processore live; `session_96k` è la quota del dominio anticipativo che la sessione di riferimento occuperebbe. Nella Fase 1 è il termine di confronto per il carico del grafo.

**`dawos-stream-test`**: leggere e scrivere tracce audio dal disco mentre il motore suonerebbe, con cinque modi di cifrare le cartelle degli utenti (le «pile»). I carichi: `w1` lettura sequenziale, `w2` sedici flussi più scritture, `w3` il diario delle modifiche con `fdatasync` (la chiamata che aspetta che il dato sia davvero sul disco).

### La campagna

**`make campaign`**: ore di misura sotto un carico che cambia a fasi, lanciate come servizio sul 285K. Il carico gira sui venti processori non live; sui quattro live gira `timerlat`.

| Fase | Che cosa carica |
| --- | --- |
| `full` | Tutto insieme: calcolo, memoria, disco, rete |
| `power` | Solo calcolo su tutti i processori non live, per portare il processore al limite di potenza (160 W) |
| `burst-a`, `burst-b` | Raffiche brevi sui P-core non live: un millisecondo di lavoro ogni tanto, o cinque ogni diciassette |
| `burst-c` | Timer e cambi di contesto a qualche kHz su quei P-core |
| `rest` | Niente |

| Colonna | Che cosa dice |
| --- | --- |
| `smi` | SMI nella fase |
| `throttle` | Interventi della protezione termica |
| `dev_irq` | Interruzioni di periferiche arrivate ai processori live: devono essere 0 |
| `above` | Risvegli sopra la soglia a cui si salva una traccia del kernel |

Il verdetto è `campaign: PASS (n of n steps)`.

## Gli strumenti di misura dentro il motore

Il motore non si limita a far passare il suono: lo genera e lo riascolta, per misurare sé stesso. Sono opzioni di `dawos-engine run`.

**Il tono** (`--tone USCITA`, `--tone-cycles N`). Una sinusoide su un'uscita. Con `--tone-cycles 3` ha esattamente tre cicli in un quanto (1500 Hz a 48 kHz): così ogni quanto è uguale al precedente, e se il segnale che torna si sposta anche di un campione solo, si vede.

**L'ascolto** (`--listen INGRESSI`). Per ogni ingresso ascoltato, secondo per secondo: il livello, il livello del tono, la **quota** (_share_: che parte della potenza del segnale è il tono, 1,0000 se c'è solo lui) e gli **slittamenti** (_slips_: quante volte il tono si è spostato nel tempo, e di quanti campioni).

**La copia** (`--route INGRESSO:USCITA`). Un ingresso copiato su un'uscita, campione per campione.

**Il conteggio** (`--count USCITA:INGRESSO`). Ogni campione suonato porta scritto il proprio numero d'ordine. Riletto su un ingresso collegato all'uscita senza convertitori, dice tre cose: la lunghezza esatta del **giro** in campioni (_way round_), ogni slittamento con il suo segno, e ogni campione tornato **fuori posto** (_out of place_). È lo strumento più severo: un campione perso, duplicato o cambiato di un bit lo fa scattare.

**Le raffiche e gli impulsi** (`--burst`, `--impulse`). Brevi segnali suonati su un'uscita e cronometrati su un ingresso: misurano il giro quando in mezzo ci sono i convertitori e un cavo, dove il conteggio non può passare.

## Il rapporto di un giro del motore

`dawos-engine run` stampa un rapporto lungo. Le parti che si guardano di più:

### La tabella dei thread

```text
slot   kind   end     cycles  missed incomplete overruns  late_avg  late_max compute_avg compute_max least_margin  behind
0      first  stopped  15012       0          0        0      1.73      2.27       75.77       86.65      1913.35       0
```

| Colonna | Che cosa dice |
| --- | --- |
| `slot` | Quale thread live: 0 è quello che tiene la scheda |
| `kind` | `first` è il thread partito all'inizio, `spare` uno di riserva mandato dal cane da guardia |
| `end` | `stopped`: fermato a fine giro; `retired`: sostituito, se n'è accorto ed è uscito; «never came back»: bloccato davvero |
| `cycles` | Cicli eseguiti |
| `missed` | Periodi passati senza essere serviti |
| `incomplete` | Cicli tagliati dal bilancio di calcolo |
| `overruns` | Cicli finiti dopo la scadenza |
| `late_avg`, `late_max` | Ritardo di risveglio, medio e massimo, in microsecondi |
| `compute_avg`, `compute_max` | Durata della fase di calcolo, media e massima, in microsecondi |
| `least_margin` | Il minimo tempo rimasto prima della scadenza; `-` se un ciclo l'ha superata |
| `behind` | Cicli partiti indietro |

Un giro sano sul 285K ha `missed`, `incomplete` e `overruns` a zero, e ritardi di risveglio di pochi microsecondi.

### Le righe della scheda

| Riga | Che cosa dice |
| --- | --- |
| `card_xruns` | Xrun, e quanti nel riscaldamento |
| `card_recoveries` | Ripartenze dopo un xrun, e la più lunga |
| `card_lost` | Volte in cui la scheda è sparita (scollegata), e se è tornata |
| `card_followers` | Per i thread che seguono la scheda: cicli presi dopo un segnale della scheda, quanti con periodi saltati, quanti risvegli a vuoto |
| `count_on_input_N` | Il giro del conteggio in campioni e in millisecondi |
| `count_slips_on_input_N` | Slittamenti del conteggio |
| `count_samples_on_input_N` | Campioni al loro posto e fuori posto |

### Le righe e le tabelle del grafo

Compaiono con `--graph FILE`. Prima ciò che il compilatore ha previsto:

| Riga | Che cosa dice |
| --- | --- |
| `graph_nodes`, `graph_buffers` | Quanti nodi e quanti buffer |
| `graph_load_of_10_ns` | Di quanti stadi e ritocchi è fatto un carico di quella classe, e quanto dovrebbe costare |
| `graph_work_us` | Il lavoro: tutti i nodi uno dopo l'altro |
| `graph_critical_path_us` | Il percorso critico, con i nomi dei nodi |
| `graph_makespan_us` | La durata prevista su quei thread, e l'accelerazione |
| `graph_budget_us` | Il bilancio (un periodo meno il margine) e se il grafo ci sta |

Poi ciò che è successo nei cicli:

```text
slot   kind    nodes_run    waits  wait_avg  wait_max    cut  starved  silent  cut_early  silent_early
1      first      270216     7210      0.58     11.54      0        0       0          0             0
```

| Colonna | Che cosa dice |
| --- | --- |
| `nodes_run` | Nodi eseguiti in tutto |
| `waits` | Volte in cui il thread ha dovuto aspettare un altro thread |
| `wait_avg`, `wait_max` | Durata media e massima di quelle attese, in microsecondi |
| `cut` | Cicli in cui il thread non è arrivato in fondo ai suoi nodi |
| `starved` | Di quelli, quanti perché l'ingresso da un altro thread non è arrivato |
| `silent` | Uscite della scheda rimaste in silenzio per un ciclo |
| `cut_early`, `silent_early` | Le stesse cose nel riscaldamento, non giudicate |

Altre righe: `graph_refused` (lucchetti trovati occupati: zero in un giro sano), `graph_peak NOME` (il picco visto da un misuratore, in dBFS), la tabella **«the nodes of the graph by kind»** (per ogni tipo di nodo: quanti, costo previsto, misurato in media, il più basso, il massimo, nanosecondi per frame), `graph_critical_path_measured_us` (il percorso critico come somma dei tempi misurati dei suoi nodi).

Con uno scambio di grafi (`--swap-graph`): `graph_changes` (scambi fatti), `graph_change_kept` (nodi che hanno proseguito con il loro stato), `graph_change_prepared_us` (tempo per leggere, compilare e costruire un grafo), `graph_change_passed_us` (dall'offerta del grafo a tutti i thread che lo eseguono).

### Il registro dei cicli e gli ingressi nel kernel

Dal registro di ogni ciclo (capitolo 2) il motore ricava **distribuzioni**: per il ritardo di risveglio, la durata del calcolo, il margine. Ogni distribuzione è una riga con minimo, media, mediana, 99 %, 99,9 %, 99,99 % e massimo. La mediana è il valore sotto cui sta metà dei cicli; «99,9 %» quello sotto cui stanno 999 cicli su mille.

| Riga | Che cosa dice |
| --- | --- |
| `wake_up_bands` | I risvegli contati per fasce: sotto 5 µs, da 5 a 10, da 10 a 20, da 20 a 50, da 50 in su. L'ultima deve essere vuota (criterio 4); la penultima si guarda caso per caso |
| `first awake to last` | Il tempo tra il primo thread sveglio e l'ultimo, nello stesso ciclo. Sul 285K meno di un microsecondo |
| `cycles_to_look_at` | I cicli anomali, ognuno con un giudizio: xrun chiesto dalla prova, xrun con il thread in ritardo, xrun con il thread in tempo, «la scheda ha avvisato tardi», «il thread ha sforato», risveglio oltre 20 µs, ciclo partito indietro |
| `syscall_dispatch: on` | Il meccanismo che ferma le chiamate di sistema nella fase di calcolo è acceso |
| `syscalls_stopped_in_compute` | Quante ne ha fermate. Zero in un giro sano; con il guasto `syscall`, una per ciclo |

Con `--kernel-entries` il motore accende una traccia del kernel sui processori live e assegna ogni evento a una **fase** del ciclo: `wake-up` (dal periodo dovuto al thread che riparte), `compute`, `delivery` (la consegna alla scheda), `wait`.

| Riga | Che cosa dice |
| --- | --- |
| `kernel_entries` | Quanti eventi letti, e se la traccia ne ha persi (`pages with records lost`: deve essere 0, o il conto non vale) |
| `kernel_entries_in_compute` | **La riga del criterio 5**: chiamate di sistema nella fase di calcolo (deve cominciare con `0 system calls`) e ingressi nel kernel non chiesti, cioè interruzioni |
| `system calls for each cycle` | Quanti cicli hanno fatto una, due, tre chiamate: con la scheda sono due per ciclo, tutte fuori dal calcolo |
| `given_the_cpu` | Nel risveglio: quante volte il thread ha avuto il processore e lo ha tenuto, e quante ha trovato occupato un lucchetto del kernel |

### I verdetti

Alla fine il motore si giudica da solo, con righe `nome: PASS (…)` o `FAIL (…)`:

| Verdetto | Che cosa giudica |
| --- | --- |
| `k04` | Con un guasto voluto: il cane da guardia se n'è accorto, ha liberato il processore e messo al lavoro la riserva entro i limiti |
| `audio` | La scheda ha fatto ciò che era chiesto: nessun xrun dopo il riscaldamento, il tono dove deve essere, il conteggio trovato, nessuno slittamento sopra il limite |
| `compute` | Nessuna chiamata di sistema nella fase di calcolo |
| `graph` | Ogni ciclo ha calcolato tutto il grafo; nessun lucchetto rifiutato; gli scambi sono andati a buon fine |

Dove i tempi non contano (macchina virtuale, opzione `--allow-xruns`) i verdetti `audio` e `graph` tollerano xrun e cicli tagliati.

## La prova audio

`dawos-audio-test` lancia il motore più volte, un **caso** alla volta, e stampa una riga per caso con il risultato e i numeri principali. Ha due modi: `loopback` sulla scheda di software, `card` sull'interfaccia vera.

| Caso | Che cosa prova |
| --- | --- |
| `plain-FREQUENZA` | Il tono esce dall'uscita 3, torna dal cavo sull'ingresso 3, viene copiato sull'uscita 5 e torna sull'ingresso 5 dentro l'interfaccia: deve essere su tutti e due, senza xrun né slittamenti |
| `kernel-entries` | Lo stesso con la traccia del kernel accesa: nessuna chiamata di sistema nella fase di calcolo |
| `syscall` | Una chiamata di sistema fatta apposta a ogni fase di calcolo: deve essere fermata, riconosciuta ed eseguita |
| `card-xrun` | Un xrun provocato: la scheda deve ripartire |
| `card-lost` | La scheda tolta e ridata (solo sulla scheda di software) |
| `stuck-node`, `stuck-follower` | Un thread che non cede mai: il cane da guardia deve sostituirlo |
| `graph` | La copia da 3 a 5 fatta da un grafo invece che dal motore |
| `relay` | Il conteggio passato da un thread all'altro attraverso quattro thread |
| `swap` | Il conteggio attraverso due grafi che si scambiano ogni secondo |

La riga `settle-FREQUENZA` non è un caso: sono otto secondi di motore per far assestare l'interfaccia dopo un cambio di frequenza.

Nella macchina virtuale, prima della riga del risultato, ci sono righe `DAWOS-AUDIO-DETAIL caso` con ciò che il motore ha detto in ogni caso: servono quando un caso fallisce e i file sono dentro una macchina ormai spenta.

## La prova della protezione

`dawos-k04-test` prova il cane da guardia senza scheda audio, cinque casi:

| Caso | Che cosa prova |
| --- | --- |
| `healthy` | Un giro sano: nessun intervento |
| `overload` | Più lavoro di quanto ci sta: i cicli vengono tagliati, ma nessun periodo perso |
| `stuck-node` | Un nodo che non ritorna: sostituzione entro i limiti |
| `stuck-thread` | Un thread bloccato fuori da ogni nodo: lo stesso |
| `watchdog-off` | Il cane da guardia fermato e poi un blocco: deve intervenire systemd |

## Gli strumenti per il carico e le forme del buffer

**`tools/audio-shapes.sh`**: il motore sulla scheda con una certa forma del buffer e un certo carico, per 400 secondi a caso di prova.

| Colonna | Che cosa dice |
| --- | --- |
| `shape` | Frame per periodo e periodi nel buffer, come `96:3` |
| `load` | La quota del periodo in cui ogni thread è tenuto occupato, in percento |
| `xruns`, `slips of the count` | Xrun e slittamenti del conteggio |
| `cut` | Cicli tagliati dal bilancio, sul thread della scheda |
| `late` | Risvegli da 20 µs in su |
| `behind` | Cicli partiti indietro |
| `left_us` | Il minimo tempo rimasto prima della scadenza |
| `ready`, `to_play` | Frame pronti in ingresso e ancora da suonare in uscita al risveglio, minimo e massimo |

**`dawos-latency-test`**: misura la latenza del giro completo con ogni forma del buffer e ogni frequenza.

**`tools/audio-hours.sh`**: giri lunghi, di un'ora, a vuoto e sotto carico.

## Gli strumenti del grafo

**`tools/graph-threads.sh`**: lo stesso grafo su uno, due, tre e quattro thread, senza scheda.

| Colonna | Che cosa dice |
| --- | --- |
| `expected_us` | La durata che il compilatore prevede su quel numero di thread |
| `slowest_avg`, `slowest_max` | La fase di calcolo del thread più lento, in media e al massimo |
| `speed_up` | L'accelerazione rispetto alla prima riga |
| `critical` | Il percorso critico misurato |
| `cut`, `missed` | Cicli tagliati e periodi persi, su tutti i thread |
| `waits`, `wait_max` | Attese tra thread, e la più lunga |
| `starved` | Nodi il cui ingresso non è mai arrivato |

**`tools/graph-calibrate.sh`**: la calibrazione del carico.

| Riga | Che cosa dice |
| --- | --- |
| `load_costs: A,B,C` | Le tre cifre misurate: un carico prima di ogni filtro, uno stadio, un ritocco, in nanosecondi per frame |
| `a_stage_in_a_cascade_of_4`, `…_of_256` | Lo stadio misurato in cascate corte e lunghe: deve essere lo stesso |
| `the_classes_with_the_figures_of_the_sources` | Quanto costano le tre classi con la calibrazione scritta nei sorgenti: devono venire 10, 40 e 150 |
| la tabella «with the figures just measured» | Le classi ricostruite con le cifre appena misurate, a 48, 96 e 192 kHz |
| `dspbench_on_cpu_N` | Lo stesso tipo di filtro nel programma di misura in C della Fase 0, per confronto |
| `denormals_taken_for_zero`, `denormals_kept` | Il costo di un carico alimentato con numeri denormalizzati, con e senza FTZ e DAZ |
