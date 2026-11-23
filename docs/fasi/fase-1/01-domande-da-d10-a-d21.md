# Fase 1 — Le domande da D-10 a D-21, e tre nuove

- **Stato**: le sette domande che servivano subito sono **decise dall'utente il 21 novembre 2026** (tabella qui sotto); le altre restano proposte, da riprendere all'apertura della fase che le usa.
- **Origine**: documento 0, sezione 4.2 («Da decidere prima della Fase 1»); punto aperto A-19; prompt della Fase 1. Il testo originale di ogni domanda resta nel documento 0: qui c'è ciò che è cambiato da allora.
- **Che cosa è cambiato da quando furono scritte**: l'[ADR 0021](../../adr/0021-core-live-senza-kernel.md) (core live senza kernel), l'[ADR 0020](../../adr/0020-plugin-linux-di-terze-parti.md) (plugin Linux di terze parti), le misure della Fase 0 e di [P1.0](P1.0-raccordo.md).

## Le decisioni del 21 novembre 2026

| Domanda | Decisione | Dove è scritta |
| --- | --- | --- |
| D-11 | Regola per fase: nessuna syscall nel calcolo; nell'attesa la chiamata bloccante e la notifica del puntatore; `futex` solo fuori dal ciclo | [ADR 0021](../../adr/0021-core-live-senza-kernel.md), precisazioni |
| D-14 | Si decide dopo la misura di P1.5, che comprende 44,1 kHz; il quanto è un parametro del motore. **Deciso il 22 novembre 2026**: tre periodi da 96 campioni a 44,1 e 48 kHz, 192 a 88,2 e 96 kHz, 384 a 176,4 e 192 kHz; i 10 ms restano fermi a ogni frequenza | Qui; [P1.5](P1.5-latenza-in-loopback.md) |
| D-17 | Tre backend: `null` in P1.2, `alsa` in P1.4, `file` in P1.7 | Qui |
| D-18 | Le regole del documento 0, con le quattro integrazioni | Qui |
| D-26 | Tutti i thread live aspettano la scheda | [ADR 0021](../../adr/0021-core-live-senza-kernel.md), precisazioni |
| D-27 | alsa-lib per cominciare; decisione definitiva in P1.8 con il conto delle syscall | Qui |
| D-28 | Sviluppo sulla 18i20 di seconda generazione, accettazione sulla quarta | Qui |
| Protezione di K-04 | Il progetto e i tempi proposti | [ADR 0023](../../adr/0023-protezione-dal-thread-che-non-cede.md) |

In tutti i casi l'utente ha scelto l'alternativa raccomandata.

## Quali servono adesso

| Domanda | Tocca | Serve la risposta |
| --- | --- | --- |
| D-11, regola sulle chiamate di sistema | Fase 1 | **Adesso** |
| D-14, R-01 a 44,1 kHz | Fase 1 | **Adesso** |
| D-17, backend audio e sviluppo fuori dall'immagine | Fase 1 | **Adesso** |
| D-18, regole per il codice Rust | Fase 1 | **Adesso** |
| D-26, come partono i thread live (nuova) | Fase 1 | **Adesso** |
| D-27, alsa-lib o interfaccia del kernel (nuova) | Fase 1 | **Adesso**, come punto di partenza |
| D-28, quale Scarlett (nuova) | Fase 1 | **Adesso** |
| D-10, passaggio tra motore e host | Fase 4, ma condiziona il grafo | Prima di P1.7 |
| D-16, carico di R-04 | Chiusa ([ADR 0011](../../adr/0011-r04-carico-sintetico.md)) | — |
| D-12, D-13, D-15, D-20, D-21 | Fase 2 | All'apertura della Fase 2 |
| D-19, trasporto MCP | Fase 2 o 3 | Più avanti |

## Le domande che servono adesso

### D-11 — Regola sulle chiamate di sistema

**Com'era**: elenco chiuso di chiamate ammesse sui thread live (`futex_wake`, `futex_wait`, attesa su ALSA), fatto rispettare in debug da un filtro seccomp.

**Che cosa è cambiato**: l'ADR 0021 divide il ciclo in due fasi e vieta ogni syscall nel calcolo, compreso il risveglio di un thread da parte di un altro.

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| **Regola per fase** | Calcolo: nessuna syscall. Attesa: la chiamata bloccante, più la notifica del puntatore sul primo thread. `futex` solo fuori dal ciclo. È ciò che l'ADR 0021 chiede e che il criterio 5 misura | Più stretta: i worker non si possono svegliare a comando |
| Elenco chiuso, come in origine | Più semplice da scrivere | Ammette `futex_wake` nel calcolo: contraddice l'ADR 0021 |

**Raccomandazione**: la regola per fase, nel testo del [progetto](P1.1-progetto-ciclo-e-k04.md), parte 1. La si fa rispettare con il meccanismo _Syscall User Dispatch_ del kernel, che distingue le fasi, al posto del filtro seccomp, che non le distingue.

### D-14 — Se R-01 non regge a 44,1 kHz

**Com'era**: con 128 campioni e due periodi, a 44,1 kHz il solo buffer vale 5,80 ms e ne restano 4,20 per convertitori e USB. Se la misura supera i 10 ms, che cosa cede?

**Che cosa è cambiato**: nulla nei numeri; non c'è ancora nessuna misura. Il criterio 1 della fase chiede 48, 96 e 192 kHz, **non 44,1**.

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| **Decidere dopo la misura di P1.5** | Si decide su un numero. A 44,1 kHz si misura comunque, anche se il criterio non lo chiede | Nessuno: la misura si fa in ogni caso |
| Fissare ora un quanto di 96 o 64 campioni come profilo «bassa latenza» | Il progetto del ciclo lo prevede dall'inizio | Si aggiunge un caso al motore prima di sapere se serve |
| Accettare ora che R-01 valga da 48 kHz in su | Il più semplice | Chi lavora a 44,1 kHz con strumenti virtuali ha una latenza che non garantiamo |

**Raccomandazione**: decidere dopo P1.5, e fare in modo che il quanto sia un parametro del motore e non una costante, così la seconda alternativa resta possibile senza riscrivere nulla. Se servirà scegliere, resto dell'idea di allora: tenere fermi i 10 ms.

**Decisione del 22 novembre 2026**, sulle misure di [P1.5](P1.5-latenza-in-loopback.md): la premessa è cambiata, perché due periodi non reggono con nessuna lunghezza. Il motore usa **tre periodi da 2 ms** (96, 192 e 384 campioni secondo la frequenza; 2,177 ms nella famiglia dei 44,1 kHz). Il giro misurato per il cavo è tra 5,5 e 7,9 ms a tutte e quattro le frequenze provate. Non c'è un profilo «bassa latenza» a parte: è la forma predefinita. Il quanto resta un parametro.

### D-17 — Backend audio alternativi e sviluppo fuori dall'immagine

**Com'era**: backend `alsa`, `null` e `file` dietro la stessa interfaccia; il motore deve compilare e superare le prove su una qualunque distribuzione Linux.

**Che cosa è cambiato**: il piano della fase ne ha bisogno prima del previsto. La protezione di K-04 si prova con il backend `null` (P1.3), prima che il motore parli con la scheda.

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| **Sì, tre backend dalla Fase 1** | Ogni modifica si prova sul PC in secondi. Il backend `file` rende le prove del grafo ripetibili bit per bit | Un'interfaccia in più da tenere pulita |
| Solo `alsa` | Meno codice | Ogni prova richiede il 285K e la Scarlett |

**Raccomandazione**: sì. `null` in P1.2, `alsa` in P1.4, `file` in P1.7 dove serve al grafo. Una precisazione: il backend `null` scandisce il ciclo con un timer, e ha quindi un percorso di risveglio diverso da quello vero. Serve alla logica e alla protezione, non alle misure di latenza.

### D-18 — Regole per il codice Rust

**Com'era**: versione stabile fissata; un solo spazio di lavoro in `crates/`; nucleo real-time in un crate separato, senza allocatore nei percorsi live e con un controllo in debug che interrompe a ogni allocazione; `unsafe` solo in moduli dedicati, con commento; dipendenze esterne approvate una per una.

**Che cosa è cambiato**: il prompt aggiunge «nessun avviso di `clippy`». L'ADR 0021 aggiunge un vincolo sulle dipendenze del nucleo real-time: nessun tipo che possa bloccare o chiamare il kernel.

**Raccomandazione**: confermare tutto, con quattro aggiunte.

- **`clippy` e `rustfmt` dentro `make lint`**, come `shellcheck`: un avviso ferma il commit.
- **Il nucleo real-time non usa** `std::sync::Mutex`, canali, `std::thread::park` né nulla che allochi: lo verifica in debug un allocatore che interrompe se chiamato da un thread live nel calcolo, e in prova il meccanismo di D-11.
- **Le dipendenze si approvano una per una, con la versione**, e si tengono in un elenco nel repository con il motivo. Per la Fase 1 ne prevedo poche: l'accesso ad alsa-lib, l'accesso alle chiamate di Linux, l'analisi della riga di comando e del file del grafo. Le propongo in P1.2 con nome e versione, dopo averle verificate.
- **La costruzione dell'immagine non scarica nulla da sé**: le dipendenze sono fissate da `Cargo.lock` e messe a disposizione prima, come per gli altri pacchetti.

### D-26 — Come partono i thread live a ogni ciclo

Nuova. L'ADR 0021 aveva lasciato aperta la scelta di chi sveglia il motore; il prompt fissa che il primo thread è svegliato da ALSA, e resta da dire come partono gli altri.

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| **A. Tutti aspettano la scheda**, ciascuno con la propria attesa | Un orologio solo. Nessuna previsione. Rispetta alla lettera l'ADR 0021 | Quattro risvegli attraverso due core; i thread partono a qualche microsecondo l'uno dall'altro |
| B. I worker hanno un timer proprio e aspettano in attesa attiva l'inizio del ciclo | I worker sono già svegli quando il ciclo comincia | Va previsto l'inizio del periodo; attesa attiva a ogni ciclo |
| C. Il primo thread sveglia gli altri | Semplice | Una syscall nel calcolo: l'ADR 0021 lo esclude |

**Raccomandazione**: A, con B pronta come ripiego se la dispersione dei risvegli misurata in P1.6 è troppa. Dettagli nel [progetto](P1.1-progetto-ciclo-e-k04.md), parte 1.

### D-27 — alsa-lib o interfaccia del kernel

Nuova. Il prompt chiede ALSA diretto in `mmap`; si può fare con la libreria o parlando direttamente con il kernel.

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| **alsa-lib sul dispositivo `hw`, per cominciare** | Negoziazione dei parametri già fatta e provata; si arriva presto all'audio | La libreria decide quando chiamare il kernel: le syscall per ciclo si scoprono misurando |
| Interfaccia del kernel da subito | Ogni syscall del ciclo è scritta da noi | Va riscritta la parte più intricata di ALSA prima di sentire un suono |

**Raccomandazione**: alsa-lib in P1.4, con il conto delle syscall per ciclo come prima misura. Si passa all'interfaccia del kernel, per il solo ciclo, se il conto supera il limite dell'ADR 0021 per colpa della libreria. La decisione definitiva sta in P1.8, con i dati.

### D-28 — Quale Scarlett, e con quale strumento per il routing

Nuova. Il prompt nomina la 18i20 e `fcp-server`. `fcp-server` governa la **quarta** generazione; oggi è collegata la **seconda**, che il kernel governa con il driver `scarlett2` e i normali controlli del mixer di ALSA.

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| **Sviluppare sulla seconda generazione, accettare sulla quarta** | Si comincia subito. Lo stesso driver audio, lo stesso numero di canali. Il routing si fissa con un file di controlli ALSA, che per la quarta diventerà la configurazione di `fcp-server` | Due configurazioni del routing da scrivere. I criteri di accettazione vanno ripetuti sulla quarta: latenza dei convertitori e firmware sono diversi |
| Aspettare la quarta generazione | Una configurazione sola | La fase resta ferma a P1.3 finché non arriva |
| Accettare la fase sulla seconda generazione | Nessuna attesa | Il prodotto è pensato per la quarta: R-01 misurato su un'altra interfaccia non è R-01 |

**Raccomandazione**: la prima. `fcp-server` entra nell'immagine quando arriva la quarta generazione; fino ad allora il passo P1.4 usa i controlli del driver `scarlett2`. Nel rapporto ogni misura dice su quale interfaccia è stata presa.

## Le domande che possono aspettare

Raccomandazione aggiornata in breve; le ripropongo per intero all'apertura della fase che le usa.

### D-10 — Modello di passaggio tra motore e host

**Raccomandazione di allora**: A, passaggio sincrono sullo stesso core, con assegnazione statica dei segmenti ai core.

**Aggiornamento**: l'ADR 0021 la rende quasi obbligata. Un host libero di girare su un core scelto dal kernel (B) richiede un risveglio tra processi dentro il calcolo, che l'ADR esclude; C resta escluso. Con A, il thread live dell'host di un plugin deve stare sveglio in attesa attiva sullo stesso core per il solo tratto in cui tocca a lui, e dormire nell'attesa del ciclo: è il caso che la contraddizione C-01 del documento 0 sollevava, e va progettato nella Fase 4. **Per la Fase 1 conta solo questo**: il grafo assegna i nodi ai core quando viene compilato, non a ogni ciclo. Lo chiedo prima di P1.7.

### D-12 — Codifica del piano di controllo e del journal

**Raccomandazione**: invariata, CBOR per socket e journal, JSON come resa per gli strumenti. Nella Fase 1 non c'è journal; il file del grafo della riga di comando è un file di prova, in un formato leggibile, e non impegna il formato del progetto.

### D-13 — Contenuto del journal

**Raccomandazione**: invariata, comando più modifiche primitive, una transazione per record. Fase 2.

### D-15 — Automazione, mappa del tempo e compensazione della latenza nell'MVP

**Raccomandazione**: invariata, sì a tutte e tre. La Fase 1 ne prepara un pezzo: la linea di ritardo tra i nodi nativi serve alla compensazione.

### D-19 — Trasporto MCP nell'MVP

**Raccomandazione**: invariata, socket Unix locale; nessuna porta di rete in ascolto. L'ADR 0019 aggiunge un vincolo: la macchina virtuale non deve poter raggiungere quel socket.

### D-20 — Rappresentazione del tempo musicale

**Raccomandazione**: invariata, tick interi. Fase 2.

### D-21 — Modello di annullamento

**Raccomandazione**: invariata, cronologia lineare unica con l'autore visibile. Fase 2.
