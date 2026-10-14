# Command & Conquer: Red Alert — in-depth source code analysis

**Language:** English (translated from the Dutch original, 14-10-2026)  
**Form:** functional description without program code  
**Goal:** to understand what the original game software does and which ideas are usable for a new RTS game in Rust.

## Reading guide and reliability

This document is based on the public EA repository for the original *Command & Conquer: Red Alert*, the repository description, and the content visible through public source files. The relevant source files include `MAP.CPP`, `BUILDING.CPP`, `DRIVE.CPP`, `BULLET.CPP`, and `TERRAIN.H`.

The analysis goes further in substance than a general description, but it is **not a complete line-by-line audit of all files**. The repository could not be fetched as a full local copy and built in this working environment. Therefore, exact values, complete mission flows, and all exceptions are not presented as verified when they do not follow directly from the source information consulted.

### Primary sources

- Official EA repository: https://github.com/electronicarts/CnC_Red_Alert
- EA's modding and source code information: https://www.ea.com/games/command-and-conquer/news/modding-faq
- Map logic: https://github.com/electronicarts/CnC_Red_Alert/blob/main/CODE/MAP.CPP
- Buildings: https://github.com/electronicarts/CnC_Red_Alert/blob/main/CODE/BUILDING.CPP
- Vehicle movement: https://github.com/electronicarts/CnC_Red_Alert/blob/main/CODE/DRIVE.CPP
- Projectiles and impacts: https://github.com/electronicarts/CnC_Red_Alert/blob/main/CODE/BULLET.CPP
- Terrain objects: https://github.com/electronicarts/CnC_Red_Alert/blob/main/CODE/TERRAIN.H

---

## 1. Summary for non-programmers

Red Alert is a real-time strategy game in which the game world is updated continuously. The player selects units, gives orders, builds a base, gathers resources, produces troops, and tries to achieve mission objectives or eliminate the opponent.

The source code shows that the game does not simply move images across a map. Every object has game state and rules. A vehicle, for example, has a position, direction, speed, order, and possibly a target. A building has, among other things, a construction or animation state, production functions, health, weapon behavior, and a relationship with the power supply. A projectile has its own movement and impact logic. The map records where objects are located and which cells are occupied or accessible.

The game is built from separate, cooperating subsystems. That makes it possible for complex gameplay to emerge from simple repeated updates: a building produces a vehicle, that vehicle receives a destination, moves across the map, looks for a valid target, attacks, and can eventually be destroyed.

**Main conclusion:** the original source code is mainly valuable as a reference for game rules, system responsibilities, and the way a classic RTS is organized. It is not a directly usable modern game engine, nor a complete collection of the original music, movies, and other game assets.

## 2. What is in the repository?

EA has made the source code public under the GPL license with additional terms. The repository has since been archived and is read-only. EA states that the current code does not fully compile and that certain old dependencies must be replaced, removed, or reimplemented.

The main structure contains:

- **CODE:** the central game code and supporting components.
- **IPX:** historical network functionality.
- **LAUNCH and LAUNCHER:** components related to starting the game and startup facilities.
- **TOOLS:** tools that belong to the code or the development process.
- **VQ and WINVQ:** components related to video processing.
- **WIN32LIB and WWFLAT32:** old Windows and graphics support.

The presence of a folder or subsystems does not prove that all original files of the game media are also in this repository. The code that plays music or movies is something different from the music and video files themselves.

### Old dependencies

EA mentions, among others, old versions of DirectX, DirectX Media, the Greenleaf Communications Library, and HMI Sound Operating System. For the original development environment, Watcom C/C++ and Borland Turbo Assembler are mentioned. The current repository is therefore mainly suited for research and restoration work by developers who can deal with historical software.

## 3. The game world: objects and state

The software manages several kinds of objects. The most important functional categories are:

- mobile units, including vehicles and infantry;
- buildings that produce, defend, or perform other functions;
- terrain objects that affect the map;
- projectiles and temporary combat objects;
- animations and effects that go with actions.

An object is not just a picture. The code must remember where it is, which owner it has, what it is currently doing, how much health it has left, and how it can cooperate with other objects.

The source code uses several specialized object types. This is visible in files such as `BUILDING.CPP`, `DRIVE.CPP`, and `BULLET.CPP`: buildings, moving vehicles, and projectiles each have their own behavior. At the same time they share general mechanisms for things such as position, map interaction, and active game state.

### Why that separation matters

A building cannot simply get the same movement rules as a tank. A projectile likewise does not follow the same rules as infantry. The game therefore needs specialized logic that builds on shared base functions.

For a new Rust game this is a useful design principle: distinguish between kinds of game objects and their behavior, without designing every object completely separately from the rest.

## 4. The map and the terrain

`MAP.CPP` contains a large part of the map logic. `TERRAIN.H` also shows that terrain objects have functions for their position, display, target coordinates, and the question of whether they can enter a map cell.

The map therefore has two roles:

1. **Visual role:** showing which terrain and which objects the player sees.
2. **Game-technical role:** determining where objects are located, which cells are occupied, and where movement or placement is allowed.

### Map cells

The game world is divided into cells. This makes it easier to record positions, to check whether a cell is accessible, and to determine which objects are in each other's way. A building generally takes up more space than one cell; mobile units must take into account their movement class and the occupancy of the map.

### Draw order

The position of an object on the map is not the same as the position at which the image is drawn on the screen. In an isometric view the draw order is important: an object may need to appear visually in front of or behind another object. The terrain and object code contains responsibilities for coordinates and sorting that fit this.

### What has not yet been fully established here

This analysis has not checked every map line individually. Therefore, the exact rules for each terrain type, all movement costs, lines of sight, bridges, water, obstacles, and special map transitions are not described as fully verified.

## 5. Player orders

The player gives orders through the interface. An order must be translated into behavior in the simulation. Conceptually that looks like this:

1. The player selects a unit or group.
2. The player chooses a target or action.
3. The game assesses whether the action is appropriate and feasible.
4. The unit stores the order or destination.
5. The unit tries to carry out the order on its own.
6. In subsequent game steps the simulation checks whether further action is needed.

An important distinction is that **receiving an order does not mean the action is immediately completed**. A unit may, for example, be en route, need to turn, wait until a cell is free, or lose a target because that target has been destroyed.

The vehicle logic in `DRIVE.CPP` supports this principle: a destination is recorded, the route can be recalculated, and the movement then proceeds on its own until the unit must stop or receives a different order.

### Design implication

In a new RTS it is wise to treat orders as a clear state. Think of: doing nothing, moving, approaching a target, attacking, loading/unloading, or returning. That prevents all logic from depending solely on the mouse click the player gave earlier.

## 6. Vehicle movement

`DRIVE.CPP` is one of the most informative files for understanding vehicle movement. The documentation in the file describes, among other things, functions for setting a destination, determining the next movement steps, turning, smooth movement, following a route, and stopping movement.

### What a vehicle has to do while moving

- Remember the desired destination.
- Determine in which direction it needs to turn.
- Follow a route or series of intermediate steps.
- Check whether the next map cell is accessible.
- Update the position step by step.
- On arrival, carry out the next action or enter a waiting state.
- React to a new order or a blockage.

The code also describes movement as a process that continues on its own after a route or turn command has been given. That limits the amount of script logic needed: the mission or order logic does not have to steer every small piece of movement itself.

### Destination and route are not the same

The destination is where the unit wants to go. The route is how it tries to get there. If the route becomes invalid or the target changes, it must be possible to determine the route again. This is an important pattern to keep in a new game as well.

### Special cases

The vehicle code also contains special handling, such as moving objects to a free nearby cell when a desired cell is not accessible, and special displacement mechanisms. Such exceptions make the original code more complex than a simple route from A to B.

### Not fully verified

This analysis does not confirm every detail of the pathfinding algorithm used, all priorities in the case of blockages, or each vehicle type individually.

## 7. Infantry and other mobile units

Infantry and vehicles share certain general game rules, but differ in how they move and deal with the map. Infantry can, for example, be distributed over cells differently than a large vehicle. The public source code contains a separate infantry component; in it, actions around orders, attacks, damage, cell occupancy, and reacting to mines are visible, among other things.

That illustrates that units are not merely variants of one image. Their type determines which rules apply.

For a new RTS it is useful to define per unit class:
- permitted terrain types;
- movement and turning behavior;
- attack capabilities;
- interaction with buildings;
- special properties;
- how the unit reacts when a cell becomes occupied.

## 8. Combat and projectiles

`BULLET.CPP` shows that projectiles have their own logic. A projectile is created with data about the type, the target, the source or responsible attacker, the strength, the type of damage, and the speed.

During flight the game handles the movement, any course adjustment toward the target, a fuse or end condition, and the moment of detonation. The source code distinguishes between, among others, projectiles that follow an arc, fall, or adjust their direction toward a target.

### What happens during an attack?

In broad terms an attack consists of:
1. A unit or building chooses a target.
2. The game checks whether the attacker can attack the target.
3. The attack is started and, if applicable, a projectile is created.
4. The projectile moves, or the attack is handled as a direct effect.
5. On impact, damage is handled.
6. The target may become damaged or destroyed.
7. The map and display are updated and effects can be shown.

The source code for projectiles explicitly describes movement, target-seeking behavior, and fuse and detonation logic. The precise damage formulas, all weapon tables, and all interactions between armor and damage type have not been fully read out in this analysis.

### Why projectiles are independent objects

A missile or shell may still be en route after being fired while the shooter does something else. The projectile therefore has its own life cycle. That makes it possible to simulate different kinds of projectiles without every attacking unit having to manage all the details itself.

## 9. Damage, destruction, and consequences

Combat changes the state of objects. A building or unit has a health state; damage can reduce it and eventually lead to destruction. Destruction has consequences for the rest of the game world: the object must disappear from the active simulation, occupied map cells must be released, and any effects must be handled.

The building code describes, among other things, destruction handling, rubble, damage display, and the removal of a building after the destruction phase has been handled. The code also shows that the visual state can change when a building is heavily damaged.

This means damage is not just a number. The state of the object also affects how it is drawn and which actions are still possible.

## 10. Buildings: much more than static objects

`BUILDING.CPP` is an extensive file and describes a large number of responsibilities. Buildings can, among other things:

- be placed and removed from the active world;
- have a construction, idle, or other animation state;
- produce units;
- attack or choose targets if they are armed;
- rotate with a turret;
- be repaired or sold;
- receive units or let them depart;
- cooperate with other objects via messages;
- affect the power supply;
- perform special functions, such as loading, unloading, charging, or launching a missile.

Not every building has all of these functions. The code uses type-dependent rules to determine what a specific building may do.

### Construction animation and game state

The building logic processes animation states and mission states in a deliberate order. The documentation describes that the animation status must be updated before certain mission actions, so that the logic reacts to a state transition at the right moment.

This is an important detail: visual animation and gameplay are coupled, but not identical. The game must prevent an animation from becoming visible only later while the associated game action should already have taken place, or the other way around.

### Production

The building code contains explicit production handling. Factories have progress and can let produced objects depart. For this the game must take into account the exit of the building and a suitable free spot on the map. Production is thus a collaboration between building, production system, and map.

### Power supply

The code adjusts the power output when the health or state of a building changes. As a result, power is not merely a visual number: damage to or loss of a building can affect the player's available energy. The precise consequences depend on the game rules and the building type.

### Repair and sale

Buildings can have a repair process and there is logic for selling or demolishing buildings. These are active gameplay processes with progress and state, not just buttons in the interface.

### Defensive buildings

Buildings with weapons can seek targets, rotate, attack, and handle their own mission behavior. The code handles the direction of turrets and special properties of different types of defense.

## 11. Economy, resources, and refineries

The building code contains specific interactions for refineries and harvesters. For example, a refinery checks whether an incoming vehicle is a type that may unload there, and communication between objects is used to determine whether an interaction is permitted.

This shows that resource gathering consists of multiple steps:
1. A harvester seeks or reaches a source.
2. It gathers resources.
3. It returns to a building that can process those resources.
4. The refinery accepts the machine when the conditions are right.
5. The gathered resources are processed into the owner's economy.
6. The machine can depart again.

The full economy is more than just a counter: objects, capacity, interactions, and conditions are needed. The building code refers, for example, to storage capacity and visually shows different fill levels of storage buildings.

**Limitation:** this analysis contains no complete list of resource prices, yields, capacities, or balance values. Those must be collected directly from all relevant data files and game rules.

## 12. Building and production AI

The building code distinguishes graphical processing from non-graphical AI. A building's AI handling processes, among other things, mission behavior, production, repair, turret rotation, special building functions, and destruction handling.

For computer-controlled buildings there is logic to determine what should be produced and to start production. That means the computer does not merely use pre-placed units: buildings can also be part of a production process.

This observation is concrete for the building AI. On its own it does not prove how the opponent's full strategic brain decides which base or attack is sensible in the long term. The complete AI strategy must be investigated separately.

## 13. Cooperation between objects

The building code uses message exchange to coordinate objects with each other. A building can, for example, check whether a vehicle may load or dock. There are also interactions around refineries, repair facilities, and landing pads.

This is a striking design pattern: objects do not always call each other's internal functions directly, but use an agreed form of communication to request an interaction and to confirm or refuse it.

For a new RTS a similar system can help with:
- vehicles seeking a repair facility;
- helicopters that need a landing pad;
- harvesters unloading resources;
- infantry boarding transport vehicles;
- buildings that accept only certain object types.

## 14. Graphical display and damage

The building code contains separate drawing logic. It determines not only which building image should be shown, but also takes into account, for example, the direction of a turret, damage, animation phase, storage level, and special effects.

A heavily damaged building can therefore be displayed differently than an intact building. A storage building can show its fill level and a factory can have its own visual layer. Game status is thereby fed back to the player.

For an RTS this is essential: the player must be able to read quickly what is happening without opening every object individually.

## 15. Sound, speech, and video

The vehicle code contains references to spoken feedback on selecting, moving, and attacking. As a result the player receives not only visual confirmation of orders, but also sound signals or voice responses.

The repository also contains old support for audio and video. But there is an important distinction:

- **Audio/video code:** determines how and when sound or video is processed.
- **Media assets:** are the actual music, speech files, sound effects, and movies.

The repository description mentions old audio and media libraries as dependencies. That does not mean all original music and movies are included in the source code repository.

For a new Rust game, sound and video can be designed as independent presentation components. Original Red Alert media must be assessed separately for availability and rights; for a game of our own, our own or properly licensed assets are the safe basis.

## 16. Network and multiplayer

The repository contains an IPX folder and refers to the Greenleaf Communications Library as an old dependency. That points to historical network functionality. It is not proof that the original network code is directly usable for modern online play.

A modern RTS has to think about:
- how players connect;
- who determines the truth about the game state;
- how latency and connection loss are handled;
- how all participants follow the same game rules;
- how cheating and inconsistencies are limited.

These topics are design choices for a new game. They cannot be derived one-to-one from the existence of the old IPX code.

## 17. Missions and scenarios

The game software must load scenario information and manage the progress of a mission. In an RTS a mission can contain specific objectives, events, and end conditions. The exact details of the complete scenario handling have not been followed file by file in this session.

For a full technical audit the following must be checked:
- how scenarios are read in;
- how objects are placed at the start;
- how triggers or events are activated;
- how the game checks whether objectives have been achieved;
- how victory, defeat, and follow-up actions are handled.

Therefore, this document does not fill in exact conditions of individual campaigns or missions without checking those source files directly.

## 18. The main loop: how everything works together

A real-time strategy game must update its world continuously. The precise full main loop has not been followed through all startup and game loop files in this analysis, but from the specialized subsystems the general pattern can be described well.

During successive game steps the game must, among other things:
1. process input and orders;
2. update the state of objects;
3. carry out movement and turning;
4. move projectiles and process impacts;
5. let AI and building missions react;
6. process production, damage, and destruction;
7. update map occupancy and visible state;
8. draw the updated game world and handle presentation.

These components affect each other. A building that is destroyed can, for example, change the power supply. A vehicle that reaches the refinery can unload resources. A projectile can damage a building, after which the building display and possibly the available energy change.

## 19. Saving, loading, and restoring game state

A complete RTS must be able to save and restore the game state when the game supports that. This concerns not only positions, but also health, orders, production progress, resources, mission progress, and mutual references between objects.

This analysis has not followed the full save/load chain through the relevant source files. Therefore it cannot be confirmed here which data are included in each type of save file or how each reference is restored.

For a new Rust game it is wise to model the game state explicitly and to design save/load before the project becomes large. Otherwise it can later be hard to restore temporary animations, ongoing orders, and production queues consistently.

## 20. What do we know concretely from the source files examined?

| Source file | Directly recognizable responsibility | Meaning in plain language |
|---|---|---|
| `MAP.CPP` | Map logic | Keeps track of the game map and map interactions |
| `TERRAIN.H` | Terrain objects | Contains terrain functions for coordinates, display, and cell access |
| `DRIVE.CPP` | Vehicle movement | Handles destinations, turning, route following, and independent movement |
| `BULLET.CPP` | Projectiles | Handles projectile movement, target-seeking behavior, and detonation |
| `BUILDING.CPP` | Building behavior | Handles, among other things, production, animations, damage, repair, power, and interactions |
| `IPX/` | Historical network | Part of old network support |
| `VQ/`, `WINVQ/` | Video processing | Support for video in the old game environment |
| `WIN32LIB/`, `WWFLAT32/` | Platform/graphics support | Link to the original Windows environment |

## 21. What is confirmed, what is an inference, and what remains to be investigated?

### Directly supported by the source information consulted
- EA has made the original Red Alert source code public.
- The repository is archived and read-only.
- The code does not fully compile without restoration work.
- Old external dependencies are needed for the original build.
- The terrain component contains logic around coordinates, display, and accessibility of map cells.
- The vehicle code handles destinations, movement, and turning.
- The projectile code handles movement and detonation.
- The building code handles, among other things, AI, production, damage, repair, power, and interactions with other objects.

### Functional inferences
- The game works by letting objects with their own state and behavior cooperate on a shared map.
- Movement, combat, and production must be processed over multiple game steps.
- Building damage can affect not only appearance but also game function.
- Separating object types and subsystems is a useful pattern for a new RTS.

### Not yet fully investigated
- The full main loop and all timing rules.
- All movement algorithms and exceptions.
- All weapon, damage, armor, and range rules.
- Complete economy and production tables.
- The strategic decision-making process of the AI.
- All triggers, scenario events, and mission end conditions.
- The full save/load implementation.
- The precise content of all original audio and video files.

## 22. Translation to a new RTS in Rust

The old code is a good reference for the kind of problems an RTS has to solve. For a new game I would separate the following subsystems.

### A. World and map
Responsible for map cells, terrain, occupancy, accessibility, and coordinates.

### B. Game objects
Units, buildings, terrain objects, projectiles, and effects. Each object type has its own rules and shares only common properties where that makes sense.

### C. Orders and missions
Handles player orders, objectives, queues, scenario events, victory, and defeat.

### D. Movement
Manages destinations, route planning, turning, blockages, movement per step, and re-planning when things change.

### E. Combat
Manages target choice, attack conditions, projectiles, impacts, damage, destruction, and visual effects.

### F. Buildings and economy
Manages construction, production, resources, power, repair, sale, loading/unloading, and production exits.

### G. AI
Separates strategic choices from the concrete execution by units and buildings.

### H. Presentation
Handles sprites or models, animations, selection, UI, sound, music, and any cutscenes.

### I. Saving and multiplayer
Store the game state in a verifiable way. Add multiplayer only with a deliberately chosen network model.

### What you should not copy literally
- The old Windows and DirectX bindings.
- The historical audio and network libraries.
- The old build environment as a basis for a modern engine.
- Code you do not understand, without first recording the associated game rule.

The new game must get its own name, world, units, graphics, music, and other recognizable content. The source code release does not automatically grant permission to use all trademarks and original media.

## 23. Recommended follow-up for a truly complete audit

For a fully reliable, file-by-file description, the remaining subsystems must be investigated in a targeted way. The order with the most value is:

1. **Startup and main loop:** how the game starts, initializes scenarios, and runs the simulation.
2. **General object structure:** how objects are created, tracked, and removed.
3. **Player input and orders:** selection, clicks, target choice, and order processing.
4. **Map and vision:** occupancy, exploration, visibility, and map refresh.
5. **Movement:** route calculation, collisions, different movement classes, and exceptions.
6. **Combat:** weapon types, target validation, damage, armor, and destruction.
7. **Economy and production:** resource flows, build conditions, costs, and production queues.
8. **AI:** strategy, tactics, target choice, and difficulty settings.
9. **Scenarios and missions:** loading, triggers, objectives, and end conditions.
10. **Presentation:** rendering, animations, sound, music, and video.
11. **Saving and loading:** which state is saved and how references are restored.
12. **Network:** what actually works and what is tied to old libraries.

For each subsystem the audit should note: which responsibility it has, which other subsystems it calls, which state it changes, which exceptions exist, and which parts cannot be built without missing dependencies.

## Final conclusion

The original Red Alert code describes an extensive simulation, not just the visible action on the screen. The map determines where objects can stand; vehicles follow orders and routes; projectiles have their own flight and impact process; buildings manage production, damage, power, and interactions; and all these components affect the state of the game world.

That is precisely the value for a new RTS in Rust: not taking over the old technology, but understanding how the game rules fit together. This analysis is more concrete than a general introduction, but remains honest about what has not yet been fully verified. A truly complete audit requires a full local copy of the repository and systematic inspection of the remaining source files.
