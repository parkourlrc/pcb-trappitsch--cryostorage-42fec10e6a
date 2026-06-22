# Workflows

## Abbreviations and notation

We use the following abbreviations for all flowcharts:

- p<sub>ch</sub>: pressure in sample chamber.
- p<sub>tr</sub>: pressure in transfer system.

The following symbols are used to describe the workflow:

```mermaid
  flowchart TD
    A[I am the task that should be accomplished]-->N
    N@{ shape: paper-tape, label: "I am a notification"} --> B
    B-->|No|Y[/Abort - I am also a task/]
    B{{I perform an instant check}}-->|Yes|tim
    tim[/"**Wait XXmin**
        I wait for the XX minutes to expire and then just continue."\]
    check_for[\"**Check for XXmin**
        I wait for a given condition to be true and then continue.
        I can also time out without the check succeeding."/]
    tim --> check_for
    check_for --> |Timeout| Y
    check_for --> |True| XX
    XX[[I am another workflow]] -->|Success| Final
    XX -->|Failure| Y
    Final[/I perform a task/]
```

## Open valves

This authorization is implemented in software. It checks the last read pressures
and then goes through the following flowchart. Workflows that open valves go
through this authorization as well.

```mermaid
  flowchart TD;
    A[Open transfer/pump valve]-->B{{0.01 < p<sub>ch</sub>/p<sub>tr</sub> < 100}};
    B-->|No| C{{p<sub>ch</sub> < 1e-5 mbar}}
    C-->|Yes| D{{p<sub>tr</sub> < 1e-5 mbar}}
    D -->|No| Y[/Refuse opening/];
    D-->|Yes| Z;
    B-->|Yes| Z[/Open valve/];
```

## Close valves

This authorization is implemented in software and in hardware on the controller
board (see [safety section](./safety.md)). Workflows that close valves go through
this workflow as well.

```mermaid
  flowchart TD;
    A[Close transfer valve] --> B{{VCT authorization}};
    B -->|No| Y[/Refuse closing/];
    B -->|Yes| Z[/Close valve/]
```

> [!NOTE]
> The pump valve can be closed without any checks.

## Start cryocooler

```mermaid
  flowchart TD
    T[Start cryocooler] --> WS{{Water safety okay}}
    WS -->|No| Err[/Abort/]
    WS -->|Yes| BS{{Baking deactivated}}
    BS -->|No| Err
    BS -->|Yes| PCH{{p<sub>ch</sub> < 1e-5mbar?}}
    PCH -->|No| Err
    PCH -->|Yes| Ok[/Start cryocooler/]
```

## Stop cryocooler

```mermaid
  flowchart TD
    T[Stop cryocooler] --> Smp{{Are any samples loaded?}}
    Smp -->|Yes| Err[/Abort/]
    Smp -->|No| Ok[/Stop cryocooler/]
```

> [!NOTE]
> This workflow can of course only check if any samples are currently set
> in the graphical user interface.
> If samples are currently set, an error will be displayed.

## Start baking

```mermaid
  flowchart TD
    T[Baking chamber] --> CC{{Cryocooler off?}}
    CC -->|No| Err[/Abort baking/]
    CC -->|Yes| P{{p<sub>ch</sub> < 1e-5mbar?}}
    P -->|No| Err
    P -->|Yes| IsOP{{Is pump valve open?}}
    IsOP -->|No| OP[[Open pump valve]]
    IsOP -->|Yes| Ok
    OP -->|Failure| Err
    OP -->|Success| Ok[/Start baking/]
```

> [!NOTE]
> Baking can be stopped without any checks.

## Venting and pumping the system

Venting and pumping the system are two workflows that are fairly complicated.
In comparison with above simpler workflows, they compare multiple timers that can
check for a condition or just wait for the timer to expire.
Both main workflows are based on several sub workflows,
which are described further down.

> [!NOTE]
> **Pump valve authorization**
>
> These workflows do not use the workflow to open the pump valve.
> Instead, they open the pump valve directly if the authorization is there.
> Here, blocks with "Pump valve authorization" check for the same authorization
> as is the case in an open pump valve workflow.

### Vent cryostorage chamber

All variables can be specified and changed in the configuration file.
These variables are:

- Minimum sample temperature to continue.
- Wait time for opening the vent valve.

The limits that are given in the "No valve authorization" block
cannot be set.
These limits are taken from the definition of the valve opening authorization.

The user has the possibility to cancel the wait time at the end.
If this is chosen, the timer will simply stop early
and continue with the workflow,
i.e., it will close the vent valve.

```mermaid
  flowchart TD
    T[Vent] --> 
    N2Not@{ shape: paper-tape, label: "Fill N<sub>2</sub> balloon."} -->|Next| CC
    CC{{Cryocooler off?}}
    CC -->|No| Err
    CC -->|Yes| TSmp
    TSmp{{Sample temperature >280K? }}
    TSmp -->|No| Err
    TSmp -->|Yes| IsOP
    IsOP{{Is pump valve open?}} -->|No| OPAuth
    OPAuth{{Pump valve authorization?}}
    OPAuth -->|Yes| OP
    OPAuth -->|No| PCL
    OP[/Open pump valve/]
    OP --> StIP
    IsOP -->|Yes| StIP
    PCHW -->|Success| VVT
    StIP[/Stop ion pump/] --> StPP
    StPP[/Stop primary pump/] --> OVV
    OVV[/Open vent valve/] --> VVT
    VVT[/Wait 25min\] --> CVV
    subgraph Level3 [Final]
    CVV[/Close vent valve/]
    Err[/Abort/]
    end
    subgraph Level2 [No valve authorization]
    %% Branch where pch >> ptr
    PCL -->|No| PCH{{p<sub>ch</sub>/p<sub>tr</sub> > 100}} 
    PCH -->|No| Err
    PCH -->|Yes| PCHW
    PCHW[[p<sub>ch</sub> >> p<sub>tr</sub>]]
    %% Branch where pch << ptr
    PCL{{p<sub>ch</sub>/p<sub>tr</sub> < 0.01}} -->|Yes| PCLW
    end
    PCLW -->|Failure| Err
    PCLW[[p<sub>ch</sub> << p<sub>tr</sub>]] -->|Success| StIP
    PCHW -->|Failure| Err
```

### Pump cryostorage chamber

The variables that can be adjusted in this workflow are two durations.

- Maximum time allowed for primary pump to pump the chamber
  down to <10<sup>-5</sup>mbar.
- Duration to wait before the ion pump is turned on.
  If this waiting time is canceled by the user, the workflow will finish
  but not turn on the ion pump.

Again, as for the venting workflow, the pressure limits
to determine if we have pump valve opening authorization
are the same as the ones for valve authorization.

```mermaid
  flowchart TD
    T[Pump] --> IsOP
    IsOP{{Is pump valve open?}} -->|No| OPAuth
    OPAuth{{Pump valve authorization?}}
    OPAuth -->|No| PCL
    OPAuth -->|Yes| OP
    OP[/Open pump valve/]
    OP --> PP
    IsOP -->|Yes| PP
    PP[/Start primary pump/] --> PCHK
    PCHK[\"Check for 40min 
        Condition: p<sub>ch</sub> < 1e-5mbar"/]
    PCHK -->|Timeout| StPP[/Stop primary pump/] --> Err
    PCHK -->|True| IPTIM
    IPTIM[/Wait 2h\] --> IP

    subgraph Level3 [Final]
      IP[/Start Ion Pump/]
      Err[/Abort/]
    end

    subgraph Level2 [No valve authorization]
      %% Branch where pch << ptr
      PCL{{p<sub>ch</sub>/p<sub>tr</sub> < 0.01}} -->|Yes| PCLW
      %% Branch where pch >> ptr
      PCL -->|No| PCH{{p<sub>ch</sub>/p<sub>tr</sub> > 100}} 
      PCH -->|No| Err
      PCH -->|Yes| PCHW
      PCHW[[p<sub>ch</sub> >> p<sub>tr</sub>]]
      CVV[/Close vent valve/]
    end
    PCLW -->|Failure| Err
    PCLW[[p<sub>ch</sub> << p<sub>tr</sub>]] -->|Success| PP
    PCHW -->|Failure| Err
    PCHW -->|Success| CVV --> PP 
```

## Equalize chamber pressure

In the case where a valve cannot be opened,
these workflows to equalize the chamber pressure can be run.
These workflows are in fact important parts in the venting and pumping workflows.

### Chamber pressure low

If the chamber pressure is too low, i.e., if
\\[ \frac{p_\text{ch}}{p_\text{tr}} < 0.01, \\]
the transfer system must first be pumped before authorization
to open valves can be given.

The limits on the pressure checks that we fulfill in this flowchart
are again the same as for authorizing the pump valve to be opened.
The only configurable quantity is the timer,
which is in the flowchart set to 20 minutes.

```mermaid
  flowchart TD
    WF[p_<sub>ch</sub> << p_<sub>tr</sub>] --> PP
    PP[/Start primary pump/] --> TIM 

    TIM[\"Check for 20min
        p<sub>ch</sub>/p<sub>tr</sub> > 0.01
        OR 
        (p<sub>ch</sub> < 1e-5mbar AND 
        p<sub>tr</sub> < 1e-5mbar)"/]
    TIM -->|Timeout| SPP[/Stop primary pump/] --> Err
    TIM -->|True| OP

    subgraph Level3 [Final]
        OP[/Open pump valve/]
        Err[/Abort/]
    end
```

### Chamber pressure high

If the chamber pressure is too high, i.e., if
\\[ \frac{p_\text{ch}}{p_\text{tr}} > 100, \\]
the transfer system might be currently pumped and must first be vented.

Again, all pressure conditions in below flowchart
come from the open valve authorization.
The only adjustable setting is the timer.

Optionally, this workflow will display a messaget to the user
to fill the nitrogen balloon for venting.
This notification is only necessary if the workflow is called
from the "pump down" workflow.
In the "vent" workflow, this message has already been given before.

> [!NOTE]
> The first steps in the following diagram turn off both pumps.
> These might in fact not even run at the moment.
> However, turning them off sets their status to off, i.e.,
> this process does not toggle their state.
> Thus, setting these to off is valid when calling this workflow
> from pumping and from venting.

```mermaid
  flowchart TD
    PR[p<sub>ch</sub> >> p<sub>tr</sub>] --> N2Not
    N2Not@{ shape: paper-tape, label: "Opt: Fill N<sub>2</sub> balloon."} -->|Next| StopIP
    StopIP[/Stop ion pump/] --> StopPP
    StopPP[/Stop primary pump/] --> OVV
    OVV[/Open vent valve/] --> TIM

    TIM[\"Check for 20min
      p<sub>ch</sub>/p<sub>tr</sub> < 100"/]
    TIM -->|Timeout| CVV[/Close vent valve/] --> Err
    TIM -->|True| Ok

    subgraph Level3 [Final]
      Ok[/Open pump valve/]
      Err[/Abort/]
    end
```

## Transfer a sample

The following workflow shows how a sample is transferred.
This workflow is fairly permissive,
such that even if the cryocooler has failed,
a user can get there sample out and keep it save.
The user is thus responsible to ensure
that the cooler is on and that the temperature of the sample holder
has reached the target temperature.

The timer to pump the gap and transfer should never be reached
and represents a maximum limit.
If the pressure is not good within these 10 minutes,
there might be a leak or something similar.

> [!NOTE]
> To ensure that the pressure is below the defined limit,
> we read it (default) 30 times in a row,
> which takes roughly 30 seconds.
> This value is configurable in the config file.

The detach check contains no timeout.
We will wait until the VCT is detached
and then close the transfer valve.
Note that the [hardware safety](./safety.md)
prevents the transfer valve from closing before the VCT valves are closed.

```mermaid
  flowchart TD
    PR[Transfer sample] --> RDY
    RDY[/Set "VCT ready" status/] --> N2Not
    N2Not@{ shape: paper-tape, label: "Fill N<sub>2</sub> balloon."} -->|Next| ClPump
    ClPump[/Close pump valve/] --> NAtt
    NAtt@{ shape: paper-tape, label: "Notify user to dock VCT and click attach button."} -->|Cancel| NRDY
    NRDY[/Unset "VCT ready" status/] --> Err
    NAtt -->|Next| CHKOP[\"Check for 10min
              VCT valves open 
              AND 
              p<sub>tr</sub> < 1e-5mbar"/]
    CHKOP -->|Timeout| NRDY
    CHKOP -->|True| NotifyOpTr
    NotifyOpTr@{ shape: paper-tape, label: "System ready. Click next to open transfer valve."} -->|Cancel| NRDY
    NotifyOpTr -->|Next| OpTrV
    OpTrV[/Open transfer valve/] --> WaitForDet
    WaitForDet[/Wait for detach: no time limit; instruct user to transfer sample, retract arm, click detach.\] -->|Cancel| NRDY
    WaitForDet --> |True| NRDYF
    NRDYF[/Unset "VCT ready" status/] --> ClTr

    subgraph Level3 [Final]
      Err[/Abort/]
      ClTr[/Close transfer valve/]
    end
```
