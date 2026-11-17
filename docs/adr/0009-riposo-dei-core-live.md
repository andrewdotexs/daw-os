# ADR 0009 — Stato di riposo dei core live

- **Stato**: accettata il 16 novembre 2026 per il 285K, sui numeri del sotto-passo P0.6b (esperimento E-01), e confermata lo stesso giorno da una misura di mezz'ora per caso
- **Data**: 13 novembre 2026
- **Origine**: D-09 e K-04 del documento 0

## Contesto

Il master prompt prescrive il solo stato di polling sui core live, tramite PM QoS. Con l'attesa attiva dei worker, questo tiene 4 P-core sempre al massimo consumo: decine di watt costanti e ventole che girano in uno studio di registrazione. D'altra parte ogni stato di riposo più profondo aggiunge latenza al risveglio.

## Decisione

1. **Sul 285K i core live riposano in C1.** Nel firmware la voce _Enhanced C-states_ sta su _Disabled_: un core fermo resta a frequenza e tensione piene.
2. **Nessuna attesa attiva**, nemmeno a motore armato: il motore lascia fermare i core live tra un ciclo e l'altro.
3. **Il profilo lo dichiara (`live_idle=c1`) e `dawos-tune` lo verifica a ogni avvio**, leggendo dal processore se il C1 viene trasformato in C1E. Se sì, il profilo non è applicato: stato `degraded`, unità fallita, avvio non buono (ADR 0018, punto 7).
4. **Il sistema legge quel valore e non lo scrive.** Rimetterlo a posto dopo un ripristino del firmware è un gesto di chi amministra la macchina.

Sono scelte dell'utente, fatte il 16 novembre 2026 sui numeri qui sotto.

## I numeri

Misurati sul 285K con `dawos-wakeup` il 16 novembre 2026: core live 4–7 a 4,6 GHz, un timer al millisecondo per core, due minuti e circa 480 000 scadenze per caso. Il ritardo è quello visto da un thread real-time in spazio utente; i due valori sono a macchina tranquilla e con gli altri core sotto carico.

| Configurazione | Ritardo medio (µs) | Ritardo massimo (µs) | Package a macchina ferma |
| --- | --- | --- | --- |
| C1E (valore di fabbrica) | 7,46 / 7,88 | 17,58 / 18,11 | 3,1 W |
| C1 | 1,79 / 1,85 | 4,05 / 4,74 | 4,2 W |
| Attesa attiva (misurata nei due avvii) | 1,57–1,73 | 3,20–5,29 | 41–42 W |

- Il risveglio da C1E costa in media 5,7–6,3 µs, e in una quota dei risvegli (dall'1,5 al 20 % secondo il core) circa 15 µs. Peggiora il massimo di 13 µs rispetto all'attesa attiva: oltre i 5 µs del criterio.
- Il risveglio da C1 costa 0,13–0,25 µs, e peggiora il massimo di circa 1 µs: meno della differenza tra due misure uguali.
- L'attesa attiva costa 37–38 W e porta il package da 34 a 57 °C a macchina altrimenti ferma.

**Conferma su mezz'ora per caso, sotto carico** (7,2 milioni di scadenze per caso, `wakeup-20261116T162952Z-lunga`): con i core live in C1 ritardo medio 1,70 µs, 99,999-esimo percentile 4,0 µs, massimo 6,74 µs; con l'attesa attiva 1,55, 3,0 e 6,22 µs. Il massimo peggiora di 0,52 µs: entro il criterio.

Il dettaglio è nella guida del sotto-passo P0.6b, parte 4; i file grezzi in `docs/misure/fase-0/raw/wakeup-20261116T145023Z-c1e-on` e `wakeup-20261116T150938Z-c1e-off`.

## Cosa si era deciso di misurare

Fino al 16 novembre 2026 questo ADR era in sospeso, con il piano qui sotto.

Quattro configurazioni sui core live, ciascuna con una prova `timerlat` di 30 minuti sotto carico:

| Caso | Stato di riposo ammesso | PM QoS |
| --- | --- | --- |
| A | solo polling | sempre attivo |
| B | fino a C1 | sempre attivo |
| C | fino a C1E | sempre attivo |
| D | predefinito del sistema | nessuno |

Per ciascuna: latenza massima e 99,999-esimo percentile, temperatura dei core, potenza del package da `turbostat`, residenza negli stati di riposo, e, se possibile, la velocità delle ventole.

## Aggiornamento del 16 novembre 2026: i casi sul 285K

I quattro casi qui sopra presuppongono che il kernel abbia più stati di riposo tra cui scegliere, e che un limite dato core per core (PM QoS) ne escluda alcuni. Sul 285K non è così _(verificato nel sotto-passo P0.6b, parte 4a)_:

- il firmware non dichiara al sistema alcuno stato di riposo: il kernel ferma un core con una sola istruzione, e il limite per core non ha nulla su cui agire;
- che cosa succeda a un core fermo lo decide una voce del firmware, _Enhanced C-states_, uguale per tutti i core: accesa (è il valore di fabbrica) il core scende a C1E, spenta resta in C1;
- stati più profondi non ci sono, per nessun core.

I casi diventano:

| Caso | Sul 285K |
| --- | --- |
| A, solo polling | Attesa attiva: un processo tiene sveglio ogni core live. È una scelta del motore, non del sistema. |
| B, fino a C1 | _Enhanced C-states: Disabled_ nel firmware. Vale per tutta la macchina. |
| C, fino a C1E | _Enhanced C-states_ acceso. Vale per tutta la macchina. |
| D, predefinito | Coincide con C. |

Si misurano con `dawos-wakeup`: ritardo di risveglio con e senza attesa attiva, a macchina tranquilla e sotto carico, più potenza e temperatura a riposo, una volta per ciascuna impostazione del firmware. Ogni caso dura due minuti, che bastano per media e minimo; massimo e 99,999-esimo percentile della configurazione scelta si confermano con una misura lunga prima di chiudere questo ADR. La residenza negli stati di riposo non si può leggere (senza driver il kernel non la conta); al suo posto c'è la quota di tempo in cui i core sono stati fermi.

Il criterio di scelta non cambia. Cambia una conseguenza: tra B e C non si sceglie per i soli core live ma per l'intera macchina, quindi nel conto entrano anche consumo e temperatura a riposo di tutti gli altri core.

## Criterio di scelta

Si adotta la configurazione più parca che rispetti entrambe le condizioni:

- latenza massima entro la soglia di 50 µs con almeno 20 µs di margine;
- peggioramento rispetto al caso A non superiore a 5 µs sulla latenza massima.

Se più configurazioni le rispettano, si tiene la più parca a motore fermo e il polling solo a motore armato.

## Aspettativa, e come è andata

L'aspettativa del 13 novembre era: C1 a motore fermo e polling a motore armato, con un'uscita da C1 di 1–2 µs. L'uscita da C1 è risultata dieci volte più rapida: il polling non serve nemmeno a motore armato.

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| C1E, senza attesa attiva | Non rispetta il criterio: +13 µs sul massimo, e un inizio di ciclo che oscilla di oltre mezzo campione a 48 kHz. |
| C1E, con attesa attiva a motore armato | Risparmia 1,1 W a macchina ferma e ne spende 37 a motore armato; e ogni volta che il motore non gira i core live tornano a costare 6 µs a risveglio. |
| C1, con attesa attiva a motore armato | 37 W e una ventina di gradi per un guadagno che le misure non distinguono dal rumore. |
| `dawos-tune` che spegne il C1E scrivendo nel registro del processore | Renderebbe la macchina indipendente dalla voce del firmware, ma scrive un registro scavalcando il kernel (che lo segnala e si contrassegna), non funzionerebbe con un kernel bloccato (D-24) e non è stato provato sul 285K. |
| Verifica che lascia solo una nota | La macchina funzionerebbe con 6 µs in più a risveglio finché qualcuno non guarda. |

## Conseguenze

- **È una scelta per l'intera macchina, non per i soli core live**: la voce del firmware è una sola. Costa 1,1 W a macchina ferma.
- **Un ripristino del firmware** (batteria, aggiornamento del firmware) riaccende il C1E. Da quel momento `dawos-tune` fallisce a ogni avvio: l'immagine già confermata continua a partire, con `tune=degraded` nel rapporto di avvio, ma un aggiornamento non viene confermato finché la voce non è rimessa su _Disabled_. La voce entra nell'elenco delle impostazioni del firmware del 285K.
- **La lettura passa dal file dei registri del processore** (`/dev/cpu/N/msr`, opzione `CONFIG_X86_MSR` del kernel). È una lettura: non è tra le operazioni che un kernel bloccato vieta _(da verificare quando si affronterà D-24)_.
- **Le misure di rumore fatte prima di questa decisione restano valide**: in `dawos-osnoise` il thread di misura non si ferma mai, e il C1E non vi entra. La campagna P0.7 si fa con il C1E spento e lo dichiara.
- **Gli stati profondi (C6 e oltre) restano fuori portata** per tutti i core: il firmware non li dichiara. Con 3–4 W di package a macchina ferma non c'è urgenza; resta un punto aperto, non di questo ADR.
- **Rischio K-04**: questa decisione non lo tocca. Un thread del motore che non cede mai il core resta il problema descritto nell'ADR 0018.

## Aggiornamento del 17 novembre 2026: il risveglio dipende dagli altri P-core

Il prezzo del risveglio da C1 su cui poggia questa decisione (0,1–0,25 µs in media, pochi microsecondi al massimo) vale con gli altri core fermi o a carico costante. Nel sotto-passo P0.6d (punto aperto A-33) si è visto un terzo caso: quando gli altri P-core fanno brevi raffiche di lavoro **a frequenza bassa**, un core live a riposo riceve l'interrupt del timer fino a 55 µs in ritardo. Un core live al lavoro, o tenuto sveglio, non ne risente.

La decisione resta: C1, senza attesa attiva. Il rimedio scelto dall'utente il 17 novembre 2026 è un altro: i P-core che non sono live hanno la frequenza fissa al massimo (ADR 0018, punto 17), e con quella il ritardo misurato è 8 µs. L'attesa attiva, che sarebbe immune per costruzione, è stata misurata di nuovo: 43 W in più.

Che cosa accada dentro il processore non è misurato. L'interpretazione che do: la tensione comune ai P-core segue quelli svegli, e un core che si risveglia mentre gli altri lavorano piano deve aspettare che risalga.

## Quando si riapre

- Se la campagna P0.7 mostra un massimo che queste misure non hanno visto.
- Con una seconda macchina supportata: i numeri sono del 285K, e un firmware che dichiara gli stati di riposo permette una scelta core per core.
- Se un aggiornamento del firmware cambia nome o comportamento alla voce.

Se la campagna P0.7, che carica i P-core non live anche a raffiche, mostra ancora risvegli lenti: allora l'attesa attiva torna in discussione.
