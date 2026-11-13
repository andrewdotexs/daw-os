# ADR 0010 — GPLv3 con eccezione per i plugin

- **Stato**: accettata; parere legale da ottenere sui punti elencati in fondo
- **Data**: 13 novembre 2026
- **Origine**: revisione del documento 0, modifica 1; sostituisce la licenza MIT e la voce M-11
- **Avvertenza**: questo ADR descrive una scelta tecnica e i suoi rischi. Non è un parere legale.

## Contesto

Il repository era nato sotto licenza MIT. La scelta è ora la GPLv3, che garantisce che dawOS e le sue versioni modificate restino liberi. Una DAW però esiste per caricare plugin, e la maggior parte dei plugin è proprietaria.

I plugin VST3 e CLAP sono librerie condivise che `dawos-plughost` carica nel proprio spazio di indirizzamento e con cui scambia strutture dati. Secondo l'interpretazione della Free Software Foundation, questo forma un'opera combinata: senza un permesso esplicito, la GPLv3 si estenderebbe al plugin, e distribuire o anche solo incoraggiare l'uso di plugin proprietari sarebbe contestabile. Il fatto che l'host sia un processo separato dal motore non cambia la questione, perché la combinazione avviene dentro l'host.

## Decisione

1. Tutto il codice di dawOS è rilasciato sotto **GNU GPL versione 3**, con identificativo SPDX `GPL-3.0-only`.
2. A questa licenza si aggiunge un **permesso supplementare secondo la sezione 7 della GPLv3**, la _dawOS Plugin Exception, versione 1.0_: è consentito caricare ed eseguire plugin con qualunque licenza attraverso le interfacce plugin di dawOS, senza che la GPL si estenda ai plugin.
3. Il testo vincolante dell'eccezione è in inglese, nel file `LICENSE`, prima del testo integrale e non modificato della GPLv3. La copia qui sotto è identica.
4. Le interfacce plugin sono VST 3 e CLAP; altre si aggiungono elencandole in `docs/PLUGIN-INTERFACES.md`, che è l'unico punto in cui il perimetro dell'eccezione si allarga.

### Perché «solo versione 3» e non «versione 3 o successive»

La richiesta era «GPLv3». Tra le due forme ho scelto quella **reversibile**: da `GPL-3.0-only` si può passare in ogni momento a `GPL-3.0-or-later` (oggi basta la tua decisione, perché sei l'unico titolare dei diritti), mentre il passaggio inverso non si può fare per le versioni già pubblicate. Inoltre l'eccezione poggia sulla sezione 7 della versione 3: come si comporterebbe con una versione futura non è prevedibile. Se preferisci «o successive», si cambia in una riga; è il punto 2 del parere legale.

### Testo dell'eccezione

```text
dawOS Plugin Exception, version 1.0
Additional permission under GNU GPL version 3, section 7

Loading a plugin into a dawOS process combines the plugin with dawOS.
Without this exception, the terms of the GNU General Public License,
version 3 ("GPLv3") would cover the whole combination.

0. Definitions.

  "dawOS" means the work to which this exception has been applied by its
  copyright holders, and any work based on it that still carries this
  exception.

  "Plugin Interface" means one of the following interfaces, as dawOS
  implements them in order to host third-party audio, MIDI and instrument
  plugins, and as published by the party that maintains the interface
  specification:

    (a) the VST 3 plug-in interfaces;
    (b) the CLAP plug-in interface;
    (c) any other plugin interface that the dawOS project lists as a
        Plugin Interface in the file docs/PLUGIN-INTERFACES.md of the
        dawOS source distribution.

  "Plugin" means a separately compiled work that

    (i)   is designed to be loaded by a host program through a Plugin
          Interface,
    (ii)  exchanges data and control with dawOS only through that Plugin
          Interface, and
    (iii) is not derived from dawOS and contains no part of dawOS, other
          than interface definitions needed to implement the Plugin
          Interface.

1. Grant of additional permission.

  As an additional permission under section 7 of GPLv3, the copyright
  holders of dawOS give you permission to load and execute Plugins through
  a Plugin Interface, to link or combine dawOS with Plugins in that way,
  and to propagate and convey the resulting combination, regardless of the
  license terms of those Plugins. The requirements of GPLv3 do not extend
  to the Plugins by reason of that combination, and the Corresponding
  Source of dawOS need not include the source code of the Plugins.

2. Limits.

  This exception does not change the terms that apply to dawOS itself:
  dawOS and every work based on it remain under GPLv3, and you must comply
  with GPLv3 for them in all other respects, as well as with the license
  of each Plugin you convey.

  This exception does not apply to a work that uses any interface of
  dawOS other than a Plugin Interface, or that uses a Plugin Interface
  which has been modified or extended beyond its published specification
  in order to expose other parts of dawOS.

  This exception covers only the parts of dawOS whose copyright holders
  have granted it. It gives no permission with respect to third-party
  code that is distributed with dawOS under terms that do not include it.

3. Modified versions.

  If you modify dawOS, you may carry this exception forward to your
  version, but you are not obliged to. GPLv3 gives permission to release a
  modified version without this exception; if you do not wish to grant it,
  delete this exception statement from your version. If your modified
  version no longer satisfies the definitions and limits above, you must
  remove this exception from it.
```

### A cosa mi sono ispirato

| Modello | Cosa ne ho preso |
| --- | --- |
| Modello della FSF per il collegamento «attraverso un'interfaccia controllata» (FAQ della GPL) | L'idea centrale: il permesso vale solo per moduli indipendenti che comunicano attraverso un'interfaccia nominata, e cade se l'interfaccia viene modificata. |
| Eccezione Classpath (OpenJDK) | La definizione di modulo indipendente come opera «non derivata» e la clausola che lascia a chi modifica la scelta di mantenere o togliere l'eccezione. |
| GCC Runtime Library Exception 3.1 | La struttura: definizioni, concessione, limiti; e la forma di «permesso supplementare secondo la sezione 7». |

Ho citato questi modelli a memoria: i testi originali vanno riletti da chi darà il parere. La formulazione finale è mia e non è stata collaudata in giudizio.

## Alternative scartate

- **MIT** (situazione precedente): massima libertà per chi riusa, nessuna garanzia che le versioni derivate restino libere.
- **GPLv3 senza eccezione**: molti progetti GPL caricano plugin proprietari senza dichiarare nulla, ma lasciano l'incertezza a utenti e produttori di plugin.
- **LGPL per il solo host dei plugin**: risolve il collegamento, ma permetterebbe anche di riusare l'host in prodotti proprietari, che non è l'intenzione.
- **AGPLv3**: aggiunge obblighi per l'uso in rete, irrilevanti per un sistema senza servizi in ascolto.

## Conseguenze

### Per il codice

- Ogni file sorgente porta l'intestazione SPDX: `GPL-3.0-only WITH AdditionRef-dawOS-plugin-exception-1.0`.
- **Nessun codice GPL di terzi nei processi che caricano plugin.** L'eccezione vale solo per il codice i cui titolari l'hanno concessa. Una libreria GPL di terzi collegata dentro `dawos-plughost` non ne sarebbe coperta, e la combinazione con un plugin proprietario non sarebbe distribuibile. Regola: in `dawos-plughost`, e in ogni futuro processo che carichi plugin, entrano solo dipendenze MIT, BSD, Apache-2.0, zlib, o LGPL collegate dinamicamente.
- **Contributi esterni.** Chi contribuisce deve concedere la stessa licenza con la stessa eccezione, altrimenti il suo codice non ne è coperto. Serve una regola scritta prima del primo contributo esterno (punto 3 del parere).

### Per le dipendenze

| Componente | Licenza | Rapporto con dawOS | Esito |
| --- | --- | --- | --- |
| VST3 SDK 3.8 | MIT _(verificato)_ | Collegato dentro l'host | Compatibile; restano le regole d'uso del marchio VST. |
| CLAP | MIT _(da verificare)_ | Collegato dentro l'host | Compatibile. |
| wgpu, egui, Smithay, crate Rust comuni | MIT o Apache-2.0 | Collegati | Compatibili: Apache-2.0 è compatibile con la GPLv3, non con la GPLv2. |
| alsa-lib, libsystemd, glibc | LGPL-2.1-or-later | Collegati dinamicamente | Compatibili. |
| `fcp-server` | GPL-3.0-or-later _(verificato)_ | Processo separato | Compatibile; riusabile come codice solo fuori dai processi che caricano plugin. |
| restic | BSD-2-Clause | Processo separato | Nessun vincolo. |
| Kernel Linux | GPL-2.0-only | Sistema separato, interfaccia di sistema | Nessun vincolo sul nostro codice in spazio utente. |
| Buildroot | GPL-2.0-or-later | Strumento di costruzione | Nessun vincolo sull'immagine prodotta; `make legal-info` genera l'elenco delle licenze. |
| Librerie solo GPL-2.0 | GPL-2.0-only | — | **Non collegabili** a codice GPLv3: vanno escluse o usate come processi separati. |
| Firmware della Scarlett | Proprietaria | File di dati | Ridistribuzione da verificare. |

Per tenere sotto controllo le licenze dei crate propongo `cargo-deny` nell'integrazione continua dalla Fase 1.

### Per l'immagine

L'immagine dawOS è un aggregato di programmi con licenze diverse; la GPLv3 di dawOS non si estende agli altri componenti e viceversa.

## Punti che richiedono un parere legale

1. **Tenuta delle definizioni.** «Plugin» e «Plugin Interface» sono abbastanza strette da impedire che qualcuno sposti funzioni di dawOS in un «plugin» proprietario? È il rischio principale di ogni eccezione di questo tipo.
2. **Solo versione 3 oppure versione 3 o successive**, e come l'eccezione si comporterebbe con una versione futura della GPL.
3. **Contributi di terzi**: basta una dichiarazione di origine (DCO) con testo esplicito sulla licenza e sull'eccezione, o serve un accordo di cessione o licenza (CLA)? Dalla risposta dipende anche la possibilità di cambiare licenza in futuro.
4. **Sezione 6 della GPLv3 e avvio verificato.** dawOS firma le UKI e lega lo sblocco dei dischi al TPM. Finché dawOS è software che l'utente installa sul proprio PC e l'utente può iscrivere le proprie chiavi, non vedo un problema. Se un giorno dawOS fosse venduto preinstallato su una macchina, la GPLv3 obbligherebbe a fornire le «informazioni di installazione» per far girare versioni modificate. Va confermato prima di qualunque distribuzione su hardware.
5. **Marchi.** «VST» è un marchio di Steinberg: la licenza MIT dell'SDK copre il codice, non il marchio. Vanno verificate le condizioni per nominarlo nell'interfaccia e nella documentazione. Lo stesso vale per «dawOS» come nome: la GPL non lo protegge.
6. **Plugin e firmware distribuiti insieme all'immagine**: se in futuro l'immagine includesse plugin o firmware di terzi, vanno verificate le loro licenze di ridistribuzione.
7. **Validità dell'eccezione nel diritto italiano ed europeo**, dato che i modelli sono nati nel diritto statunitense.
8. **Espressione SPDX.** `AdditionRef-` è la forma prevista dalla specifica SPDX 3.0 per eccezioni non registrate _(da verificare)_; alcuni strumenti potrebbero non riconoscerla.

## Nota sul file `LICENSE`

Il file contiene un preambolo e l'eccezione prima del testo della GPLv3. Il riconoscimento automatico della licenza di GitHub potrebbe non classificarlo come GPL-3.0. Se dà fastidio, l'alternativa convenzionale è tenere il testo puro della GPLv3 in `COPYING` e l'eccezione in `LICENSE`.

## Quando si riapre

All'arrivo del parere legale, al primo contributo esterno, e a ogni nuova interfaccia plugin.
