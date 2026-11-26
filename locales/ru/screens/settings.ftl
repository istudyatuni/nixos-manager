settings-interface-section = Интерфейс
settings-language = Язык интерфейса
settings-language-system = Как в системе
settings-language-english = English
settings-language-russian = Русский
settings-scale = Масштаб интерфейса
settings-scale-percent = { $percent }%
settings-repository-section = Репозиторий
settings-repository = Ссылка на репозиторий GitHub
settings-repository-placeholder = https://github.com/owner/repository
settings-repository-invalid-hint = Введите корректную ссылку
settings-repository-https-hint = Укажите HTTPS-ссылку на репозиторий GitHub
settings-repository-domain-hint = Укажите ссылку с доменом github.com
settings-repository-path-hint = Укажите ссылку в формате https://github.com/owner/repo
settings-repository-checking-hint = Проверяем доступ к репозиторию на GitHub…
settings-repository-unavailable-anonymous-hint = Репозиторий недоступен. Проверьте ссылку или введите токен
settings-repository-unavailable-hint = Репозиторий недоступен. Проверьте ссылку и права токена
settings-repository-network-hint = Не удалось подключиться к GitHub
settings-repository-timeout-hint =
    GitHub не ответил за { $seconds } { $seconds ->
        [one] секунду
        [few] секунды
       *[other] секунд
    }
settings-repository-check-failed-hint = Не удалось проверить доступ к репозиторию через Git
settings-token = Токен доступа GitHub
settings-token-hint = Необязательно для публичного репозитория
settings-token-invalid-hint = Укажите токен доступа GitHub без пробелов
settings-token-rejected-hint = GitHub отклонил токен. Проверьте его или введите новый
settings-path = Локальный путь
settings-path-placeholder = { $path }
settings-path-hint = Приложение синхронизирует локальную ветку с GitHub и заменяет коммиты, которые есть только в локальной ветке
settings-branch = Ветка
settings-branch-placeholder = main
settings-branch-invalid-hint = Укажите корректное имя ветки Git
settings-branch-load-remote = Показать все ветки с GitHub
settings-branch-loading-remote = Загружаем ветки с GitHub…
settings-branch-loading-remote-tooltip = { settings-branch-loading-remote }
settings-branch-loaded-remote-tooltip = Ветки с GitHub загружены
settings-branch-loading-local = Читаем список локальных веток…
settings-branch-network-warning = Не удалось подключиться к GitHub. В списке только локальные ветки.
settings-branch-unavailable-hint = Не удалось загрузить ветки с GitHub
settings-branch-head-unavailable = Не удалось определить ветку по умолчанию. Укажите ветку вручную.
settings-flake-directory = Поддиректория с flake.nix
settings-flake-directory-placeholder = (корневая директория)
settings-flake-directory-hint = Путь относительно репозитория
settings-cancel = Отмена
settings-save = Сохранить
settings-saving = Сохранение…
settings-operation-in-progress = Настройки можно будет сохранить после завершения текущей операции.
settings-error-path = Укажите корректный локальный путь.
settings-error-flake-directory = Укажите относительный путь без компонентов «..».
settings-error-write = Не удалось сохранить настройки. Проверьте права на запись в { $directory }.
