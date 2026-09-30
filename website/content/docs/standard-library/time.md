---
title: Time
description: Current sleep support and the status of clock and calendar APIs.
status: Planned
---
There is no `std/time` module in the current source tree. General clock, duration, date and calendar APIs are not defined by this reference.

## Sleep today

The existing process module provides `os.sleep(milliseconds: int)`, a blocking sleep operation. Import `"std/os"` to use it.

```skuld
import "std/os"

func main() {
    os.sleep(10)
    print("Done")
}
```

## Blocking behavior

Sleeping blocks the current execution. It is not a scheduler, asynchronous timer or concurrency primitive.

## Future scope

A dedicated time module requires separate API and platform decisions. No release date or final spelling is promised here.
