# ADR 0023 — Protezione dal thread live che non cede

- **Stato**: accettata dall'utente il 21 novembre 2026 come progetto; realizzata e verificata lo stesso giorno sul 285K con la prova d'urto del [passo P1.3](../fasi/fase-1/P1.3-ciclo-e-protezione.md). Restano da fare il passo 4 del sorvegliante (P1.7) e la misura del margine (P1.6)
- **Data**: 21 novembre 2026
- **Origine**: rischio K-04 del documento 0; punti aperti A-06, A-07 e A-18; [ADR 0021](0021-core-live-senza-kernel.md), punto 4; prompt della Fase 1; [progetto P1.1](../fasi/fase-1/P1.1-progetto-ciclo-e-k04.md), parte 2

## Contesto

Sui core live il _fair server_ del kernel è spento ([ADR 0018](0018-profili-di-avvio-e-regolazione.md)): il suo timer raggiungeva ogni core live una volta al secondo. Il prezzo è noto dall'inizio: un thread real-time che non dorme mai toglie il core ai thread del kernel legati a quella CPU, e alla lunga ferma la macchina.

L'ADR 0021 ha fissato che il motore cede il core a ogni ciclo, nella fase di attesa, e che smette di calcolare a un margine fisso dalla fine del periodo. Questo copre il caso di un grafo troppo pesante. Non copre un nodo che non ritorna, né un difetto del motore: lì il thread non arriva mai all'attesa.

Il prompt della Fase 1 chiede che la protezione viva nel motore, che il sorvegliante giri fuori dai core live, e che il progetto preceda il codice.

## Decisione

**Tre linee, dalla più leggera alla più drastica.**

### 1. Il bilancio di calcolo

Ogni thread live confronta l'ora con la scadenza del calcolo tra un nodo e l'altro, senza syscall. Alla scadenza non prende altri nodi; il primo thread consegna ciò che è pronto e silenzio per il resto; il ciclo è contato come incompleto; tutti vanno all'attesa.

- **Margine di partenza**: 150 µs prima della fine del quanto.
- **Il margine deve superare il nodo più lungo**, finché il controllo non conosce la durata attesa di ogni nodo (dal passo P1.7). La misura del passo P1.3 non ha potuto dare il valore: con il backend `null` non c'è nulla da consegnare dopo il calcolo. Si misura con la scheda, nel passo P1.6 (punto aperto A-54).

### 2. Il sorvegliante

Un thread del motore sui core del dominio `system`, con priorità real-time più alta di tutto ciò che gira lì. A ogni quanto legge, per ogni thread live, il numero del ciclo e l'istante di ingresso nel calcolo, che il thread scrive in memoria.

**Un thread nel calcolo da più di tre quanti è bloccato.** Il sorvegliante allora, nell'ordine:

1. gli toglie la priorità real-time e lo sposta sui core del dominio `system`, a priorità minima;
2. segna il nodo in esecuzione come abbandonato: da subito produce silenzio;
3. porta sul core liberato un thread di riserva, creato all'avvio e parcheggiato fuori dai core live, e lo sveglia;
4. chiede al thread di controllo un grafo senza quel nodo;
5. segnala: registro di sistema, contatore, stato «degradato» del motore.

**Il thread bloccato non si termina**: resta isolato fino al riavvio del motore.

### 3. La seconda linea

systemd sorveglia il motore: il thread delle statistiche segnala di essere vivo a intervalli, e se smette il processo viene terminato e riavviato.

**Il _fair server_ resta spento** sui core live.

### I tempi

| Evento | Tempo dal blocco | Misurato sul 285K (P1.3) |
| --- | --- | --- |
| Il ciclo in corso si chiude senza quel nodo | Meno di 2,7 ms | — |
| Il sorvegliante rileva il blocco | Entro 11 ms | 8,00 ms |
| Il core live torna ai thread del kernel | Entro 15 ms | 8,03 ms |
| Il thread di riserva è al lavoro | Entro 50 ms | 10,67 ms |
| Il grafo senza il nodo è in uso | Entro 500 ms | Da fare in P1.7 |
| Riavvio della macchina | Mai | Nessuno |

Durante il blocco un lavoro chiesto al kernel su ogni core è rimasto in coda al più 6,68 ms.

## Perché

- **Il sorvegliante da fuori, non un limite del kernel sul thread**: un limite di tempo di CPU per thread arma un timer sul core live, cioè proprio un ingresso nel kernel durante il calcolo.
- **Isolare e non terminare**: in un processo solo, uccidere un thread lascia la memoria in uno stato che nessuno conosce. Spostato su un altro dominio a priorità minima, non fa danni.
- **Il thread di riserva**: senza, un nodo difettoso toglierebbe un quarto della capacità live fino al riavvio.
- **Il margine non protegge i thread del kernel** _(misurato in P1.3)_: un thread che sfora il periodo dorme comunque fino al seguente, e il lavoro del kernel resta entro 0,28 ms. Il margine protegge la consegna dell'audio.
- **Tre quanti**: un ciclo sano dura meno di uno, e il bilancio ferma a meno di uno anche un ciclo pesante. Quindici millisecondi di fame per i thread del kernel di un core sono lontani da ogni soglia che il kernel sorveglia, che sono dell'ordine dei secondi.

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| Soglia di rilevamento più lunga, dieci quanti | Più distanza dai falsi allarmi, che però tre quanti già escludono; i thread del kernel restano fermi più a lungo |
| Riavviare il motore al primo blocco, senza thread di riserva | Più semplice, ma l'audio si interrompe del tutto per un difetto che riguarda un nodo solo |
| Riaccendere il _fair server_ come cintura di sicurezza | Un interrupt al secondo su ogni core live _(misurato in P0.6a)_, per un caso che le linee 2 e 3 coprono |
| Limite di tempo di CPU del kernel sul thread (`RLIMIT_RTTIME`) | Arma un timer sul core live |
| Terminare il thread bloccato | Stato della memoria ignoto; in Rust non c'è un modo sicuro |

## Conseguenze

- **Il motore ha un thread di riserva per ogni core live**, e memoria preallocata per ciascuno.
- **Ogni nodo deve poter essere abbandonato**: il grafo deve saper proseguire trattandolo come silenzio, e ciò che il nodo possedeva non viene più toccato.
- **Nella Fase 4** i nodi saranno plugin dentro processi host: il passo 1 diventa «declassa, poi termina l'host», che lì è possibile. I thread da sorvegliare comprenderanno quelli creati dai plugin ([ADR 0020](0020-plugin-linux-di-terze-parti.md)).
- **La prova d'urto è un comando del motore**: nodo che non termina, thread che non cede, grafo troppo pesante, sorvegliante fermo. Sostituisce la terza variante di `dawos-starve-test` (A-07).
- **Si prova sul 285K**: in macchina virtuale, con il _fair server_ acceso, un processo interrotto dentro il kernel non è più ripartito (A-06), e quel difetto riguarda proprio questo genere di prova.

## Quando si riapre

- Se spostare da fuori un thread bloccato su un core live non riesce sempre. Nella prova d'urto di P1.3 è riuscito due volte su due, in 30 µs; la prova si ripete nella campagna del passo P1.10.
- Se il margine del bilancio misurato risulta così grande da togliere capacità utile ai core live.
- Se compaiono falsi allarmi con la soglia di tre quanti.
- All'apertura della Fase 4, per i processi host e i thread dei plugin.
