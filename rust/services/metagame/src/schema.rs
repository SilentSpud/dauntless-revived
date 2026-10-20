// @generated automatically by Diesel CLI.

diesel::table! {
    account_bans (account_id) {
        account_id -> Text,
        reason -> Text,
        address -> Nullable<Text>,
        active -> BigInt,
        updated_at -> BigInt,
        actor -> Text,
        expires_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    account_device_signals (account_id, kind) {
        account_id -> Text,
        kind -> Text,
        signal_hash -> Text,
        observed_at -> BigInt,
    }
}

diesel::table! {
    accountkeyrecovery (userId) {
        userId -> Text,
        keyHash -> Text,
        ciphertext -> Text,
    }
}

diesel::table! {
    authepochs (userId) {
        userId -> Text,
        epoch -> BigInt,
    }
}

diesel::table! {
    blocks (blockerId, blockedId) {
        blockerId -> Text,
        blockedId -> Text,
        createdAt -> BigInt,
    }
}

diesel::table! {
    bounties (accountId, bountyId) {
        accountId -> Text,
        bountyId -> Text,
        slotIndex -> Nullable<BigInt>,
        updateVersion -> BigInt,
        data -> Text,
        updatedDate -> Text,
    }
}

diesel::table! {
    bountydraft (accountId) {
        accountId -> Text,
        data -> Text,
        updatedDate -> Text,
    }
}

diesel::table! {
    breadcrumbs (characterId) {
        characterId -> Text,
        userId -> Text,
        #[sql_name = "breadcrumbs"]
        breadcrumbs_ -> Text,
        updateVersion -> BigInt,
    }
}

diesel::table! {
    characterhistory (id) {
        id -> BigInt,
        characterId -> Text,
        userId -> Text,
        updateVersion -> BigInt,
        name -> Text,
        data -> Text,
        savedDate -> Text,
        reason -> Text,
    }
}

diesel::table! {
    characters (characterId) {
        characterId -> Text,
        userId -> Text,
        createdDate -> Text,
        lastModifiedDate -> Text,
        name -> Text,
        updateVersion -> BigInt,
        data -> Text,
    }
}

diesel::table! {
    cooldowns (accountId, cooldownId) {
        accountId -> Text,
        cooldownId -> Text,
        startedDate -> Text,
        updatedDate -> Text,
    }
}

diesel::table! {
    device_ban_signals (account_id, kind) {
        account_id -> Text,
        kind -> Text,
        signal_hash -> Text,
    }
}

diesel::table! {
    discordlinks (discordId) {
        discordId -> Text,
        userId -> Text,
        linkedAt -> Text,
    }
}

diesel::table! {
    encounteredcontent (characterId) {
        characterId -> Text,
        userId -> Text,
        #[sql_name = "encounteredcontent"]
        encounteredcontent_ -> Text,
    }
}

diesel::table! {
    entitlements (accountId, name) {
        accountId -> Text,
        name -> Text,
        activatedDate -> Text,
        duration -> BigInt,
        source -> Text,
        grantedDate -> Text,
        revokedDate -> Nullable<Text>,
    }
}

diesel::table! {
    escalationprogression (accountId, seasonId) {
        accountId -> Text,
        seasonId -> Text,
        level -> BigInt,
        xp -> BigInt,
        updateVersion -> BigInt,
        contentHash -> Text,
        updatedDate -> Text,
    }
}

diesel::table! {
    escalationtalents (accountId, seasonId, talentId) {
        accountId -> Text,
        seasonId -> Text,
        talentId -> Text,
        rank -> BigInt,
    }
}

diesel::table! {
    escalationunlocks (accountId, seasonId, unlockId) {
        accountId -> Text,
        seasonId -> Text,
        unlockId -> Text,
        collectedDate -> Text,
    }
}

diesel::table! {
    friendships (userLow, userHigh) {
        userLow -> Text,
        userHigh -> Text,
        requesterId -> Text,
        status -> Text,
        createdAt -> BigInt,
        updatedAt -> BigInt,
    }
}

diesel::table! {
    gameserverapikeys (id) {
        id -> BigInt,
        keyHash -> Nullable<Text>,
    }
}

diesel::table! {
    gameserverapikeystoregister (key) {
        key -> Text,
    }
}

diesel::table! {
    guildinvites (inviteId) {
        inviteId -> Text,
        guildId -> Text,
        inviteeId -> Text,
        inviterId -> Text,
        createdAt -> BigInt,
        expiresAt -> BigInt,
    }
}

diesel::table! {
    guildmembers (accountId) {
        accountId -> Text,
        guildId -> Text,
        rank -> Text,
        joinedAt -> BigInt,
        updatedAt -> BigInt,
    }
}

diesel::table! {
    guilds (guildId) {
        guildId -> Text,
        name -> Text,
        nameKey -> Text,
        nameplate -> Text,
        nameplateKey -> Nullable<Text>,
        leaderId -> Text,
        createdAt -> BigInt,
        updatedAt -> BigInt,
    }
}

diesel::table! {
    huntpassselection (accountId) {
        accountId -> Text,
        progressionId -> Text,
        updatedDate -> Text,
    }
}

diesel::table! {
    huntregions (userId) {
        userId -> Text,
        region -> Text,
    }
}

diesel::table! {
    inventories (characterId) {
        characterId -> Text,
        instancedItems -> Text,
        stackedItems -> Text,
    }
}

diesel::table! {
    inventorylog (id) {
        id -> BigInt,
        time -> Text,
        userId -> Text,
        characterId -> Text,
        transactionId -> Nullable<Text>,
        source -> Nullable<Text>,
        caller -> Text,
        operation -> Text,
        catalogId -> Nullable<Text>,
        instanceId -> Nullable<Text>,
        quantityChange -> Nullable<BigInt>,
        quantityAfter -> Nullable<BigInt>,
        updateVersion -> Nullable<BigInt>,
    }
}

diesel::table! {
    inventorytransactions (id) {
        id -> BigInt,
        transactionId -> Text,
        characterId -> Text,
        userId -> Text,
        requestHash -> Text,
        status -> BigInt,
        response -> Text,
        createdDate -> Text,
    }
}

diesel::table! {
    invitecodes (invitecode) {
        invitecode -> Text,
        usesRemaining -> BigInt,
        infiniteUses -> Bool,
    }
}

diesel::table! {
    leaderboardprofiles (accountId) {
        accountId -> Text,
        epicId -> Text,
        platformId -> Text,
        platform -> Text,
        displayName -> Text,
        updatedDate -> Text,
    }
}

diesel::table! {
    loadouthistory (id) {
        id -> BigInt,
        characterId -> Text,
        userId -> Text,
        version -> BigInt,
        loadouts -> Text,
        persistent -> Text,
        savedDate -> Text,
        reason -> Text,
    }
}

diesel::table! {
    loadouts (characterId) {
        characterId -> Text,
        userId -> Text,
        #[sql_name = "loadouts"]
        loadouts_ -> Text,
        persistent -> Text,
    }
}

diesel::table! {
    loadoutslots (characterId) {
        characterId -> Text,
        userId -> Text,
        numCharacterSlots -> BigInt,
        activeIndex -> BigInt,
        updatedDate -> Text,
    }
}

diesel::table! {
    moderation_events (id) {
        id -> Nullable<BigInt>,
        account_id -> Text,
        reason -> Text,
        address -> Nullable<Text>,
        active -> BigInt,
        created_at -> BigInt,
        actor -> Text,
        expires_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    objectives (accountId, objectiveId) {
        accountId -> Text,
        objectiveId -> Text,
        progress -> BigInt,
        completedCount -> BigInt,
        createdDate -> Text,
        lastModifiedDate -> Text,
    }
}

diesel::table! {
    onetimejobs (jobId) {
        jobId -> Text,
        doneDate -> Text,
    }
}

diesel::table! {
    player_addresses (account_id, address) {
        account_id -> Text,
        address -> Text,
        last_seen -> BigInt,
    }
}

diesel::table! {
    progress_tracks (accountId, progressionId) {
        accountId -> Text,
        progressionId -> Text,
        progress -> BigInt,
        confirmedFreeRank -> BigInt,
        confirmedPremiumRank -> BigInt,
        confirmedDate -> Text,
        updatedDate -> Text,
    }
}

diesel::table! {
    progression_events (id) {
        id -> BigInt,
        time -> Text,
        accountId -> Text,
        caller -> Text,
        route -> Text,
        body -> Nullable<Text>,
        status -> BigInt,
        reply -> Nullable<Text>,
        note -> Nullable<Text>,
    }
}

diesel::table! {
    recoverychallenges (tokenHash) {
        tokenHash -> Text,
        discordId -> Text,
        userId -> Nullable<Text>,
        keyHash -> Nullable<Text>,
        expiresAt -> BigInt,
        used -> BigInt,
    }
}

diesel::table! {
    recoveryevents (id) {
        id -> Nullable<BigInt>,
        discordId -> Text,
        action -> Text,
        createdAt -> BigInt,
    }
}

diesel::table! {
    recoveryproofs (discordId) {
        discordId -> Text,
        userId -> Text,
        keyHash -> Text,
    }
}

diesel::table! {
    slayerlinkinvites (inviteId) {
        inviteId -> Text,
        senderId -> Text,
        targetId -> Text,
        senderSlot -> BigInt,
        createdAt -> BigInt,
        expiresAt -> BigInt,
        status -> Text,
    }
}

diesel::table! {
    slayerlinks (linkId) {
        linkId -> Text,
        senderId -> Text,
        targetId -> Text,
        senderSlot -> BigInt,
        targetSlot -> BigInt,
        createdAt -> BigInt,
        endsAt -> BigInt,
    }
}

diesel::table! {
    storepurchases (tokenHash) {
        tokenHash -> Text,
        accountId -> Text,
        characterId -> Text,
        skuId -> Text,
        offerHash -> Text,
        createdDate -> Text,
        expiresDate -> Text,
        redeemedDate -> Nullable<Text>,
    }
}

diesel::table! {
    trialruns (id) {
        id -> BigInt,
        trialId -> Text,
        trialWeek -> BigInt,
        difficulty -> BigInt,
        mode -> Text,
        runKey -> Text,
        groupKey -> Text,
        completionTime -> BigInt,
        objectivesCompleted -> BigInt,
        sessionId -> Text,
        entries -> Text,
        submittedDate -> Text,
    }
}

diesel::table! {
    trialweeks (week) {
        week -> BigInt,
        finalizedDate -> Text,
        awardedAccounts -> BigInt,
    }
}

diesel::table! {
    userapikeys (userId) {
        userId -> Text,
        keyHash -> Text,
    }
}

diesel::table! {
    userapikeystoregister (userId) {
        userId -> Text,
        key -> Text,
    }
}

diesel::table! {
    users (userId) {
        userId -> Text,
        name -> Text,
        notes -> BigInt,
        isAdmin -> Bool,
    }
}

diesel::joinable!(discordlinks -> users (userId));
diesel::joinable!(huntregions -> users (userId));

diesel::allow_tables_to_appear_in_same_query!(
    account_bans,
    account_device_signals,
    accountkeyrecovery,
    authepochs,
    blocks,
    bounties,
    bountydraft,
    breadcrumbs,
    characterhistory,
    characters,
    cooldowns,
    device_ban_signals,
    discordlinks,
    encounteredcontent,
    entitlements,
    escalationprogression,
    escalationtalents,
    escalationunlocks,
    friendships,
    gameserverapikeys,
    gameserverapikeystoregister,
    guildinvites,
    guildmembers,
    guilds,
    huntpassselection,
    huntregions,
    inventories,
    inventorylog,
    inventorytransactions,
    invitecodes,
    leaderboardprofiles,
    loadouthistory,
    loadouts,
    loadoutslots,
    moderation_events,
    objectives,
    onetimejobs,
    player_addresses,
    progress_tracks,
    progression_events,
    recoverychallenges,
    recoveryevents,
    recoveryproofs,
    slayerlinkinvites,
    slayerlinks,
    storepurchases,
    trialruns,
    trialweeks,
    userapikeys,
    userapikeystoregister,
    users,
);
