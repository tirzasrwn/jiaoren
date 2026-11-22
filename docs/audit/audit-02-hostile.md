# Markdown Renderer Audit: Part 2 (Hostile and Malformed Input)

This file starts with a UTF-8 BOM and uses mixed line endings. **EXPECT:** the app never crashes, hangs, or corrupts
the rest of the page. Each section is independent; scroll to the end to confirm the final marker `END-OF-PART-2` appears.

---

## H1. Deep nesting
T-HO-001: 60-level blockquote. EXPECT: renders or caps depth gracefully.
> > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > > deep quote

T-HO-002: 40-level nested list.
- level 1
  - level 2
    - level 3
      - level 4
        - level 5
          - level 6
            - level 7
              - level 8
                - level 9
                  - level 10
                    - level 11
                      - level 12
                        - level 13
                          - level 14
                            - level 15
                              - level 16
                                - level 17
                                  - level 18
                                    - level 19
                                      - level 20
                                        - level 21
                                          - level 22
                                            - level 23
                                              - level 24
                                                - level 25
                                                  - level 26
                                                    - level 27
                                                      - level 28
                                                        - level 29
                                                          - level 30
                                                            - level 31
                                                              - level 32
                                                                - level 33
                                                                  - level 34
                                                                    - level 35
                                                                      - level 36
                                                                        - level 37
                                                                          - level 38
                                                                            - level 39
                                                                              - level 40

T-HO-003: Deeply nested emphasis (regex backtracking check).

********************************************************************************************************************************************************************************************************text********************************************************************************************************************************************************************************************************

_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a_a

[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[x]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]

![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![![x](aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)

```````````````````````````````````````````````````````````````````````````````````````````````````` x ```````````````````````````````````````````````````````````````````````````````````````````````````

<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<

T-HO-004: ReDoS probes. EXPECT: each returns instantly.

aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa!

- - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - 

| a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a | a |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x |

[link](((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((x)))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))

<https://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa>

www.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.com

## H2. Control and odd characters

T-HO-010: NUL, BEL, ESC, DEL, and C1 controls follow (invisible): a bcdefg

T-HO-011: Lone surrogates / noncharacters: ￾ ￿ ﷐ and replacement char �

T-HO-012: Bidi override attack. EXPECT: not visually reordering surrounding UI: ‮abc.exe‬ and ⁦isolate⁩

T-HO-013: Line/paragraph separators: a b c

T-HO-014: Homoglyph link text (Cyrillic a): [pаypal.com](https://evil.example/)

T-HO-015: Zero-width in URL: [x](https://exam​ple.com)

## H3. Line endings

T-HO-020: CRLF line
second CRLF line

- crlf list a
- crlf list b

```js
const crlf = true;
```

T-HO-021: Bare CR linesecond bare CR line
T-HO-022: Mixed endings in one fence:

```text
line LF
line CRLF
line CRend
```

## H4. Malformed structures (all closed again so later content survives)

T-HO-030: Unclosed inline HTML, comments and CDATA.

<div class="never-closed"
<!-- unclosed comment? no, closed -->
<![CDATA[ cdata block ]]>
<?php echo "processing instruction"; ?>
<!DOCTYPE html>

T-HO-031: Broken table delimiter rows.

| a | b |
|--|
| 1 | 2 |

| a | b |
| :-: | -:- |
| 1 | 2 |

T-HO-032: Link reference loops and self reference.

[a]: <b>
[b]: <a>
[loop]: #loop

[a] [b] [loop]

T-HO-033: Footnote referencing itself and cyclic definitions.

Text[^x] and[^y].

[^x]: Refers to y[^y]
[^y]: Refers to x[^x]

T-HO-034: Heading id collision with generated ids.

### duplicate-1
### duplicate
### duplicate
### duplicate-1

[to dup](#duplicate-1)

T-HO-035: Heading id that collides with app-internal DOM ids. EXPECT: no DOM clobbering of app UI.

### root
### app
### __proto__
### constructor
### toString
### nodeType

<a id="root" name="root"></a><div id="app"></div><form id="document"><input name="cookie"></form>

T-HO-036: Prototype pollution via front-matter-like blocks and attributes.

```json
{"__proto__": {"polluted": true}, "constructor": {"prototype": {"polluted2": true}}}
```

After render, console check: `({}).polluted === undefined && ({}).polluted2 === undefined`. EXPECT: true.

## H5. Mermaid hostile cases

T-HO-040: Extremely wide flowchart (200 nodes in one rank).

```mermaid
flowchart LR
    R --> N0
    R --> N1
    R --> N2
    R --> N3
    R --> N4
    R --> N5
    R --> N6
    R --> N7
    R --> N8
    R --> N9
    R --> N10
    R --> N11
    R --> N12
    R --> N13
    R --> N14
    R --> N15
    R --> N16
    R --> N17
    R --> N18
    R --> N19
    R --> N20
    R --> N21
    R --> N22
    R --> N23
    R --> N24
    R --> N25
    R --> N26
    R --> N27
    R --> N28
    R --> N29
    R --> N30
    R --> N31
    R --> N32
    R --> N33
    R --> N34
    R --> N35
    R --> N36
    R --> N37
    R --> N38
    R --> N39
    R --> N40
    R --> N41
    R --> N42
    R --> N43
    R --> N44
    R --> N45
    R --> N46
    R --> N47
    R --> N48
    R --> N49
    R --> N50
    R --> N51
    R --> N52
    R --> N53
    R --> N54
    R --> N55
    R --> N56
    R --> N57
    R --> N58
    R --> N59
    R --> N60
    R --> N61
    R --> N62
    R --> N63
    R --> N64
    R --> N65
    R --> N66
    R --> N67
    R --> N68
    R --> N69
    R --> N70
    R --> N71
    R --> N72
    R --> N73
    R --> N74
    R --> N75
    R --> N76
    R --> N77
    R --> N78
    R --> N79
    R --> N80
    R --> N81
    R --> N82
    R --> N83
    R --> N84
    R --> N85
    R --> N86
    R --> N87
    R --> N88
    R --> N89
    R --> N90
    R --> N91
    R --> N92
    R --> N93
    R --> N94
    R --> N95
    R --> N96
    R --> N97
    R --> N98
    R --> N99
    R --> N100
    R --> N101
    R --> N102
    R --> N103
    R --> N104
    R --> N105
    R --> N106
    R --> N107
    R --> N108
    R --> N109
    R --> N110
    R --> N111
    R --> N112
    R --> N113
    R --> N114
    R --> N115
    R --> N116
    R --> N117
    R --> N118
    R --> N119
    R --> N120
    R --> N121
    R --> N122
    R --> N123
    R --> N124
    R --> N125
    R --> N126
    R --> N127
    R --> N128
    R --> N129
    R --> N130
    R --> N131
    R --> N132
    R --> N133
    R --> N134
    R --> N135
    R --> N136
    R --> N137
    R --> N138
    R --> N139
    R --> N140
    R --> N141
    R --> N142
    R --> N143
    R --> N144
    R --> N145
    R --> N146
    R --> N147
    R --> N148
    R --> N149
    R --> N150
    R --> N151
    R --> N152
    R --> N153
    R --> N154
    R --> N155
    R --> N156
    R --> N157
    R --> N158
    R --> N159
    R --> N160
    R --> N161
    R --> N162
    R --> N163
    R --> N164
    R --> N165
    R --> N166
    R --> N167
    R --> N168
    R --> N169
    R --> N170
    R --> N171
    R --> N172
    R --> N173
    R --> N174
    R --> N175
    R --> N176
    R --> N177
    R --> N178
    R --> N179
    R --> N180
    R --> N181
    R --> N182
    R --> N183
    R --> N184
    R --> N185
    R --> N186
    R --> N187
    R --> N188
    R --> N189
    R --> N190
    R --> N191
    R --> N192
    R --> N193
    R --> N194
    R --> N195
    R --> N196
    R --> N197
    R --> N198
    R --> N199
```

T-HO-041: Long chain (300 nodes). EXPECT: renders or errors inside the container, UI stays responsive.

```mermaid
flowchart TD
    C0 --> C1
    C1 --> C2
    C2 --> C3
    C3 --> C4
    C4 --> C5
    C5 --> C6
    C6 --> C7
    C7 --> C8
    C8 --> C9
    C9 --> C10
    C10 --> C11
    C11 --> C12
    C12 --> C13
    C13 --> C14
    C14 --> C15
    C15 --> C16
    C16 --> C17
    C17 --> C18
    C18 --> C19
    C19 --> C20
    C20 --> C21
    C21 --> C22
    C22 --> C23
    C23 --> C24
    C24 --> C25
    C25 --> C26
    C26 --> C27
    C27 --> C28
    C28 --> C29
    C29 --> C30
    C30 --> C31
    C31 --> C32
    C32 --> C33
    C33 --> C34
    C34 --> C35
    C35 --> C36
    C36 --> C37
    C37 --> C38
    C38 --> C39
    C39 --> C40
    C40 --> C41
    C41 --> C42
    C42 --> C43
    C43 --> C44
    C44 --> C45
    C45 --> C46
    C46 --> C47
    C47 --> C48
    C48 --> C49
    C49 --> C50
    C50 --> C51
    C51 --> C52
    C52 --> C53
    C53 --> C54
    C54 --> C55
    C55 --> C56
    C56 --> C57
    C57 --> C58
    C58 --> C59
    C59 --> C60
    C60 --> C61
    C61 --> C62
    C62 --> C63
    C63 --> C64
    C64 --> C65
    C65 --> C66
    C66 --> C67
    C67 --> C68
    C68 --> C69
    C69 --> C70
    C70 --> C71
    C71 --> C72
    C72 --> C73
    C73 --> C74
    C74 --> C75
    C75 --> C76
    C76 --> C77
    C77 --> C78
    C78 --> C79
    C79 --> C80
    C80 --> C81
    C81 --> C82
    C82 --> C83
    C83 --> C84
    C84 --> C85
    C85 --> C86
    C86 --> C87
    C87 --> C88
    C88 --> C89
    C89 --> C90
    C90 --> C91
    C91 --> C92
    C92 --> C93
    C93 --> C94
    C94 --> C95
    C95 --> C96
    C96 --> C97
    C97 --> C98
    C98 --> C99
    C99 --> C100
    C100 --> C101
    C101 --> C102
    C102 --> C103
    C103 --> C104
    C104 --> C105
    C105 --> C106
    C106 --> C107
    C107 --> C108
    C108 --> C109
    C109 --> C110
    C110 --> C111
    C111 --> C112
    C112 --> C113
    C113 --> C114
    C114 --> C115
    C115 --> C116
    C116 --> C117
    C117 --> C118
    C118 --> C119
    C119 --> C120
    C120 --> C121
    C121 --> C122
    C122 --> C123
    C123 --> C124
    C124 --> C125
    C125 --> C126
    C126 --> C127
    C127 --> C128
    C128 --> C129
    C129 --> C130
    C130 --> C131
    C131 --> C132
    C132 --> C133
    C133 --> C134
    C134 --> C135
    C135 --> C136
    C136 --> C137
    C137 --> C138
    C138 --> C139
    C139 --> C140
    C140 --> C141
    C141 --> C142
    C142 --> C143
    C143 --> C144
    C144 --> C145
    C145 --> C146
    C146 --> C147
    C147 --> C148
    C148 --> C149
    C149 --> C150
    C150 --> C151
    C151 --> C152
    C152 --> C153
    C153 --> C154
    C154 --> C155
    C155 --> C156
    C156 --> C157
    C157 --> C158
    C158 --> C159
    C159 --> C160
    C160 --> C161
    C161 --> C162
    C162 --> C163
    C163 --> C164
    C164 --> C165
    C165 --> C166
    C166 --> C167
    C167 --> C168
    C168 --> C169
    C169 --> C170
    C170 --> C171
    C171 --> C172
    C172 --> C173
    C173 --> C174
    C174 --> C175
    C175 --> C176
    C176 --> C177
    C177 --> C178
    C178 --> C179
    C179 --> C180
    C180 --> C181
    C181 --> C182
    C182 --> C183
    C183 --> C184
    C184 --> C185
    C185 --> C186
    C186 --> C187
    C187 --> C188
    C188 --> C189
    C189 --> C190
    C190 --> C191
    C191 --> C192
    C192 --> C193
    C193 --> C194
    C194 --> C195
    C195 --> C196
    C196 --> C197
    C197 --> C198
    C198 --> C199
    C199 --> C200
    C200 --> C201
    C201 --> C202
    C202 --> C203
    C203 --> C204
    C204 --> C205
    C205 --> C206
    C206 --> C207
    C207 --> C208
    C208 --> C209
    C209 --> C210
    C210 --> C211
    C211 --> C212
    C212 --> C213
    C213 --> C214
    C214 --> C215
    C215 --> C216
    C216 --> C217
    C217 --> C218
    C218 --> C219
    C219 --> C220
    C220 --> C221
    C221 --> C222
    C222 --> C223
    C223 --> C224
    C224 --> C225
    C225 --> C226
    C226 --> C227
    C227 --> C228
    C228 --> C229
    C229 --> C230
    C230 --> C231
    C231 --> C232
    C232 --> C233
    C233 --> C234
    C234 --> C235
    C235 --> C236
    C236 --> C237
    C237 --> C238
    C238 --> C239
    C239 --> C240
    C240 --> C241
    C241 --> C242
    C242 --> C243
    C243 --> C244
    C244 --> C245
    C245 --> C246
    C246 --> C247
    C247 --> C248
    C248 --> C249
    C249 --> C250
    C250 --> C251
    C251 --> C252
    C252 --> C253
    C253 --> C254
    C254 --> C255
    C255 --> C256
    C256 --> C257
    C257 --> C258
    C258 --> C259
    C259 --> C260
    C260 --> C261
    C261 --> C262
    C262 --> C263
    C263 --> C264
    C264 --> C265
    C265 --> C266
    C266 --> C267
    C267 --> C268
    C268 --> C269
    C269 --> C270
    C270 --> C271
    C271 --> C272
    C272 --> C273
    C273 --> C274
    C274 --> C275
    C275 --> C276
    C276 --> C277
    C277 --> C278
    C278 --> C279
    C279 --> C280
    C280 --> C281
    C281 --> C282
    C282 --> C283
    C283 --> C284
    C284 --> C285
    C285 --> C286
    C286 --> C287
    C287 --> C288
    C288 --> C289
    C289 --> C290
    C290 --> C291
    C291 --> C292
    C292 --> C293
    C293 --> C294
    C294 --> C295
    C295 --> C296
    C296 --> C297
    C297 --> C298
    C298 --> C299
    C299 --> C300
```

T-HO-042: maxTextSize / maxEdges limits. EXPECT: Mermaid's own limit error, handled.

```mermaid
flowchart LR
    X0 --> Y0
    X1 --> Y1
    X2 --> Y2
    X3 --> Y3
    X4 --> Y4
    X5 --> Y5
    X6 --> Y6
    X7 --> Y7
    X8 --> Y8
    X9 --> Y9
    X10 --> Y10
    X11 --> Y11
    X12 --> Y12
    X13 --> Y13
    X14 --> Y14
    X15 --> Y15
    X16 --> Y16
    X17 --> Y17
    X18 --> Y18
    X19 --> Y19
    X20 --> Y20
    X21 --> Y21
    X22 --> Y22
    X23 --> Y23
    X24 --> Y24
    X25 --> Y25
    X26 --> Y26
    X27 --> Y27
    X28 --> Y28
    X29 --> Y29
    X30 --> Y30
    X31 --> Y31
    X32 --> Y32
    X33 --> Y33
    X34 --> Y34
    X35 --> Y35
    X36 --> Y36
    X37 --> Y37
    X38 --> Y38
    X39 --> Y39
    X40 --> Y40
    X41 --> Y41
    X42 --> Y42
    X43 --> Y43
    X44 --> Y44
    X45 --> Y45
    X46 --> Y46
    X47 --> Y47
    X48 --> Y48
    X49 --> Y49
    X50 --> Y50
    X51 --> Y51
    X52 --> Y52
    X53 --> Y53
    X54 --> Y54
    X55 --> Y55
    X56 --> Y56
    X57 --> Y57
    X58 --> Y58
    X59 --> Y59
    X60 --> Y60
    X61 --> Y61
    X62 --> Y62
    X63 --> Y63
    X64 --> Y64
    X65 --> Y65
    X66 --> Y66
    X67 --> Y67
    X68 --> Y68
    X69 --> Y69
    X70 --> Y70
    X71 --> Y71
    X72 --> Y72
    X73 --> Y73
    X74 --> Y74
    X75 --> Y75
    X76 --> Y76
    X77 --> Y77
    X78 --> Y78
    X79 --> Y79
    X80 --> Y80
    X81 --> Y81
    X82 --> Y82
    X83 --> Y83
    X84 --> Y84
    X85 --> Y85
    X86 --> Y86
    X87 --> Y87
    X88 --> Y88
    X89 --> Y89
    X90 --> Y90
    X91 --> Y91
    X92 --> Y92
    X93 --> Y93
    X94 --> Y94
    X95 --> Y95
    X96 --> Y96
    X97 --> Y97
    X98 --> Y98
    X99 --> Y99
    X100 --> Y100
    X101 --> Y101
    X102 --> Y102
    X103 --> Y103
    X104 --> Y104
    X105 --> Y105
    X106 --> Y106
    X107 --> Y107
    X108 --> Y108
    X109 --> Y109
    X110 --> Y110
    X111 --> Y111
    X112 --> Y112
    X113 --> Y113
    X114 --> Y114
    X115 --> Y115
    X116 --> Y116
    X117 --> Y117
    X118 --> Y118
    X119 --> Y119
    X120 --> Y120
    X121 --> Y121
    X122 --> Y122
    X123 --> Y123
    X124 --> Y124
    X125 --> Y125
    X126 --> Y126
    X127 --> Y127
    X128 --> Y128
    X129 --> Y129
    X130 --> Y130
    X131 --> Y131
    X132 --> Y132
    X133 --> Y133
    X134 --> Y134
    X135 --> Y135
    X136 --> Y136
    X137 --> Y137
    X138 --> Y138
    X139 --> Y139
    X140 --> Y140
    X141 --> Y141
    X142 --> Y142
    X143 --> Y143
    X144 --> Y144
    X145 --> Y145
    X146 --> Y146
    X147 --> Y147
    X148 --> Y148
    X149 --> Y149
    X150 --> Y150
    X151 --> Y151
    X152 --> Y152
    X153 --> Y153
    X154 --> Y154
    X155 --> Y155
    X156 --> Y156
    X157 --> Y157
    X158 --> Y158
    X159 --> Y159
    X160 --> Y160
    X161 --> Y161
    X162 --> Y162
    X163 --> Y163
    X164 --> Y164
    X165 --> Y165
    X166 --> Y166
    X167 --> Y167
    X168 --> Y168
    X169 --> Y169
    X170 --> Y170
    X171 --> Y171
    X172 --> Y172
    X173 --> Y173
    X174 --> Y174
    X175 --> Y175
    X176 --> Y176
    X177 --> Y177
    X178 --> Y178
    X179 --> Y179
    X180 --> Y180
    X181 --> Y181
    X182 --> Y182
    X183 --> Y183
    X184 --> Y184
    X185 --> Y185
    X186 --> Y186
    X187 --> Y187
    X188 --> Y188
    X189 --> Y189
    X190 --> Y190
    X191 --> Y191
    X192 --> Y192
    X193 --> Y193
    X194 --> Y194
    X195 --> Y195
    X196 --> Y196
    X197 --> Y197
    X198 --> Y198
    X199 --> Y199
    X200 --> Y200
    X201 --> Y201
    X202 --> Y202
    X203 --> Y203
    X204 --> Y204
    X205 --> Y205
    X206 --> Y206
    X207 --> Y207
    X208 --> Y208
    X209 --> Y209
    X210 --> Y210
    X211 --> Y211
    X212 --> Y212
    X213 --> Y213
    X214 --> Y214
    X215 --> Y215
    X216 --> Y216
    X217 --> Y217
    X218 --> Y218
    X219 --> Y219
    X220 --> Y220
    X221 --> Y221
    X222 --> Y222
    X223 --> Y223
    X224 --> Y224
    X225 --> Y225
    X226 --> Y226
    X227 --> Y227
    X228 --> Y228
    X229 --> Y229
    X230 --> Y230
    X231 --> Y231
    X232 --> Y232
    X233 --> Y233
    X234 --> Y234
    X235 --> Y235
    X236 --> Y236
    X237 --> Y237
    X238 --> Y238
    X239 --> Y239
    X240 --> Y240
    X241 --> Y241
    X242 --> Y242
    X243 --> Y243
    X244 --> Y244
    X245 --> Y245
    X246 --> Y246
    X247 --> Y247
    X248 --> Y248
    X249 --> Y249
    X250 --> Y250
    X251 --> Y251
    X252 --> Y252
    X253 --> Y253
    X254 --> Y254
    X255 --> Y255
    X256 --> Y256
    X257 --> Y257
    X258 --> Y258
    X259 --> Y259
    X260 --> Y260
    X261 --> Y261
    X262 --> Y262
    X263 --> Y263
    X264 --> Y264
    X265 --> Y265
    X266 --> Y266
    X267 --> Y267
    X268 --> Y268
    X269 --> Y269
    X270 --> Y270
    X271 --> Y271
    X272 --> Y272
    X273 --> Y273
    X274 --> Y274
    X275 --> Y275
    X276 --> Y276
    X277 --> Y277
    X278 --> Y278
    X279 --> Y279
    X280 --> Y280
    X281 --> Y281
    X282 --> Y282
    X283 --> Y283
    X284 --> Y284
    X285 --> Y285
    X286 --> Y286
    X287 --> Y287
    X288 --> Y288
    X289 --> Y289
    X290 --> Y290
    X291 --> Y291
    X292 --> Y292
    X293 --> Y293
    X294 --> Y294
    X295 --> Y295
    X296 --> Y296
    X297 --> Y297
    X298 --> Y298
    X299 --> Y299
    X300 --> Y300
    X301 --> Y301
    X302 --> Y302
    X303 --> Y303
    X304 --> Y304
    X305 --> Y305
    X306 --> Y306
    X307 --> Y307
    X308 --> Y308
    X309 --> Y309
    X310 --> Y310
    X311 --> Y311
    X312 --> Y312
    X313 --> Y313
    X314 --> Y314
    X315 --> Y315
    X316 --> Y316
    X317 --> Y317
    X318 --> Y318
    X319 --> Y319
    X320 --> Y320
    X321 --> Y321
    X322 --> Y322
    X323 --> Y323
    X324 --> Y324
    X325 --> Y325
    X326 --> Y326
    X327 --> Y327
    X328 --> Y328
    X329 --> Y329
    X330 --> Y330
    X331 --> Y331
    X332 --> Y332
    X333 --> Y333
    X334 --> Y334
    X335 --> Y335
    X336 --> Y336
    X337 --> Y337
    X338 --> Y338
    X339 --> Y339
    X340 --> Y340
    X341 --> Y341
    X342 --> Y342
    X343 --> Y343
    X344 --> Y344
    X345 --> Y345
    X346 --> Y346
    X347 --> Y347
    X348 --> Y348
    X349 --> Y349
    X350 --> Y350
    X351 --> Y351
    X352 --> Y352
    X353 --> Y353
    X354 --> Y354
    X355 --> Y355
    X356 --> Y356
    X357 --> Y357
    X358 --> Y358
    X359 --> Y359
    X360 --> Y360
    X361 --> Y361
    X362 --> Y362
    X363 --> Y363
    X364 --> Y364
    X365 --> Y365
    X366 --> Y366
    X367 --> Y367
    X368 --> Y368
    X369 --> Y369
    X370 --> Y370
    X371 --> Y371
    X372 --> Y372
    X373 --> Y373
    X374 --> Y374
    X375 --> Y375
    X376 --> Y376
    X377 --> Y377
    X378 --> Y378
    X379 --> Y379
    X380 --> Y380
    X381 --> Y381
    X382 --> Y382
    X383 --> Y383
    X384 --> Y384
    X385 --> Y385
    X386 --> Y386
    X387 --> Y387
    X388 --> Y388
    X389 --> Y389
    X390 --> Y390
    X391 --> Y391
    X392 --> Y392
    X393 --> Y393
    X394 --> Y394
    X395 --> Y395
    X396 --> Y396
    X397 --> Y397
    X398 --> Y398
    X399 --> Y399
    X400 --> Y400
    X401 --> Y401
    X402 --> Y402
    X403 --> Y403
    X404 --> Y404
    X405 --> Y405
    X406 --> Y406
    X407 --> Y407
    X408 --> Y408
    X409 --> Y409
    X410 --> Y410
    X411 --> Y411
    X412 --> Y412
    X413 --> Y413
    X414 --> Y414
    X415 --> Y415
    X416 --> Y416
    X417 --> Y417
    X418 --> Y418
    X419 --> Y419
    X420 --> Y420
    X421 --> Y421
    X422 --> Y422
    X423 --> Y423
    X424 --> Y424
    X425 --> Y425
    X426 --> Y426
    X427 --> Y427
    X428 --> Y428
    X429 --> Y429
    X430 --> Y430
    X431 --> Y431
    X432 --> Y432
    X433 --> Y433
    X434 --> Y434
    X435 --> Y435
    X436 --> Y436
    X437 --> Y437
    X438 --> Y438
    X439 --> Y439
    X440 --> Y440
    X441 --> Y441
    X442 --> Y442
    X443 --> Y443
    X444 --> Y444
    X445 --> Y445
    X446 --> Y446
    X447 --> Y447
    X448 --> Y448
    X449 --> Y449
    X450 --> Y450
    X451 --> Y451
    X452 --> Y452
    X453 --> Y453
    X454 --> Y454
    X455 --> Y455
    X456 --> Y456
    X457 --> Y457
    X458 --> Y458
    X459 --> Y459
    X460 --> Y460
    X461 --> Y461
    X462 --> Y462
    X463 --> Y463
    X464 --> Y464
    X465 --> Y465
    X466 --> Y466
    X467 --> Y467
    X468 --> Y468
    X469 --> Y469
    X470 --> Y470
    X471 --> Y471
    X472 --> Y472
    X473 --> Y473
    X474 --> Y474
    X475 --> Y475
    X476 --> Y476
    X477 --> Y477
    X478 --> Y478
    X479 --> Y479
    X480 --> Y480
    X481 --> Y481
    X482 --> Y482
    X483 --> Y483
    X484 --> Y484
    X485 --> Y485
    X486 --> Y486
    X487 --> Y487
    X488 --> Y488
    X489 --> Y489
    X490 --> Y490
    X491 --> Y491
    X492 --> Y492
    X493 --> Y493
    X494 --> Y494
    X495 --> Y495
    X496 --> Y496
    X497 --> Y497
    X498 --> Y498
    X499 --> Y499
    X500 --> Y500
    X501 --> Y501
    X502 --> Y502
    X503 --> Y503
    X504 --> Y504
    X505 --> Y505
    X506 --> Y506
    X507 --> Y507
    X508 --> Y508
    X509 --> Y509
    X510 --> Y510
    X511 --> Y511
    X512 --> Y512
    X513 --> Y513
    X514 --> Y514
    X515 --> Y515
    X516 --> Y516
    X517 --> Y517
    X518 --> Y518
    X519 --> Y519
    X520 --> Y520
    X521 --> Y521
    X522 --> Y522
    X523 --> Y523
    X524 --> Y524
    X525 --> Y525
    X526 --> Y526
    X527 --> Y527
    X528 --> Y528
    X529 --> Y529
    X530 --> Y530
    X531 --> Y531
    X532 --> Y532
    X533 --> Y533
    X534 --> Y534
    X535 --> Y535
    X536 --> Y536
    X537 --> Y537
    X538 --> Y538
    X539 --> Y539
    X540 --> Y540
    X541 --> Y541
    X542 --> Y542
    X543 --> Y543
    X544 --> Y544
    X545 --> Y545
    X546 --> Y546
    X547 --> Y547
    X548 --> Y548
    X549 --> Y549
    X550 --> Y550
    X551 --> Y551
    X552 --> Y552
    X553 --> Y553
    X554 --> Y554
    X555 --> Y555
    X556 --> Y556
    X557 --> Y557
    X558 --> Y558
    X559 --> Y559
    X560 --> Y560
    X561 --> Y561
    X562 --> Y562
    X563 --> Y563
    X564 --> Y564
    X565 --> Y565
    X566 --> Y566
    X567 --> Y567
    X568 --> Y568
    X569 --> Y569
    X570 --> Y570
    X571 --> Y571
    X572 --> Y572
    X573 --> Y573
    X574 --> Y574
    X575 --> Y575
    X576 --> Y576
    X577 --> Y577
    X578 --> Y578
    X579 --> Y579
    X580 --> Y580
    X581 --> Y581
    X582 --> Y582
    X583 --> Y583
    X584 --> Y584
    X585 --> Y585
    X586 --> Y586
    X587 --> Y587
    X588 --> Y588
    X589 --> Y589
    X590 --> Y590
    X591 --> Y591
    X592 --> Y592
    X593 --> Y593
    X594 --> Y594
    X595 --> Y595
    X596 --> Y596
    X597 --> Y597
    X598 --> Y598
    X599 --> Y599
```

T-HO-043: Cyclic everything.

```mermaid
flowchart LR
    A --> B --> C --> A
    A --> A
    subgraph s1
        s2
    end
    subgraph s2
        s1
    end
```

T-HO-044: Subgraph with same id as a node, and reserved words.

```mermaid
flowchart LR
    end --> start
    graph --> subgraph
    class --> click
    default --> style
```

T-HO-045: Sequence diagram with 40 participants.

```mermaid
sequenceDiagram
    participant P0
    participant P1
    participant P2
    participant P3
    participant P4
    participant P5
    participant P6
    participant P7
    participant P8
    participant P9
    participant P10
    participant P11
    participant P12
    participant P13
    participant P14
    participant P15
    participant P16
    participant P17
    participant P18
    participant P19
    participant P20
    participant P21
    participant P22
    participant P23
    participant P24
    participant P25
    participant P26
    participant P27
    participant P28
    participant P29
    participant P30
    participant P31
    participant P32
    participant P33
    participant P34
    participant P35
    participant P36
    participant P37
    participant P38
    participant P39
    P0->>P1: msg 0
    P1->>P2: msg 1
    P2->>P3: msg 2
    P3->>P4: msg 3
    P4->>P5: msg 4
    P5->>P6: msg 5
    P6->>P7: msg 6
    P7->>P8: msg 7
    P8->>P9: msg 8
    P9->>P10: msg 9
    P10->>P11: msg 10
    P11->>P12: msg 11
    P12->>P13: msg 12
    P13->>P14: msg 13
    P14->>P15: msg 14
    P15->>P16: msg 15
    P16->>P17: msg 16
    P17->>P18: msg 17
    P18->>P19: msg 18
    P19->>P20: msg 19
    P20->>P21: msg 20
    P21->>P22: msg 21
    P22->>P23: msg 22
    P23->>P24: msg 23
    P24->>P25: msg 24
    P25->>P26: msg 25
    P26->>P27: msg 26
    P27->>P28: msg 27
    P28->>P29: msg 28
    P29->>P30: msg 29
    P30->>P31: msg 30
    P31->>P32: msg 31
    P32->>P33: msg 32
    P33->>P34: msg 33
    P34->>P35: msg 34
    P35->>P36: msg 35
    P36->>P37: msg 36
    P37->>P38: msg 37
    P38->>P39: msg 38
```

T-HO-046: Unterminated frontmatter in a Mermaid block.

```mermaid
---
title: never closed
flowchart LR
    A --> B
```

T-HO-047: Prototype-name node ids.

```mermaid
flowchart LR
    __proto__ --> constructor
    constructor --> prototype
    prototype --> toString
```

T-HO-048: Mermaid fence immediately followed by text with no blank line.

```mermaid
flowchart LR
    Tight1 --> Tight2
```
Text directly after fence.
- list directly after paragraph

## H6. Fence torture (must be last structural section)

T-HO-050: Fence closed by a LONGER fence and with trailing spaces.

```js
const a = 1;
`````   

T-HO-051: Fence with info string containing backticks is NOT a fence (for backtick fences).

``` `not a fence` 
still a paragraph? EXPECT: per CommonMark, this line is not a code fence opener.

T-HO-052: Fence interrupted by a list marker inside (should stay code).

```text
- looks like a list
# looks like a heading
> looks like a quote
```

T-HO-053: An unclosed fence at EOF swallows the rest of the document (per CommonMark). The next marker is INSIDE this block on purpose:

```text
This fence is intentionally never closed.
END-OF-PART-2 (EXPECT: displayed inside a code block, and the app still renders fine)
